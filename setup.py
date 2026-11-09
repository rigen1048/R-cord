#!/usr/bin/env python3
"""Rcord setup: build release, ship as AppImage, install desktop integration.

Usage: python3 setup.py [--skip-desktop-install]

Steps:
  1. Build frontend (trunk, release) + backend (cargo, release)
  2. Assemble AppDir bundling the GTK/WebKit runtime from the userland
     sysroot (so the AppImage runs on machines without webkit dev packages)
  3. Pack AppDir into Rcord-x86_64.AppImage via appimagetool
  4. Install desktop icon + launcher entry (XDG) pointing at the AppImage
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SYSROOT = Path("/opt/data/opt/tauri-sys")
SYSLIB = SYSROOT / "usr/lib/x86_64-linux-gnu"
DIST = ROOT / "package"
APPDIR = DIST / "AppDir"
APP_NAME = "Rcord"
APP_ID = "rcord"
APPIMAGE_TOOL = DIST / "appimagetool-x86_64.AppImage"
APPIMAGE_TOOL_URL = (
    "https://github.com/AppImage/appimagetool/releases/download/continuous/"
    "appimagetool-x86_64.AppImage"
)

# glibc core: expected from the host at runtime, never bundled
EXCLUDE_LIBS = {
    "linux-vdso.so.1", "ld-linux-x86-64.so.2",
    "libc.so.6", "libm.so.6", "libdl.so.2", "libpthread.so.0",
    "librt.so.1", "libresolv.so.2", "libnss_files.so.2",
    "libnss_dns.so.2", "libutil.so.1",
}


def env():
    e = dict(os.environ)
    e["PKG_CONFIG_PATH"] = f"{SYSLIB}/pkgconfig:{SYSROOT}/usr/share/pkgconfig"
    e["PKG_CONFIG_SYSROOT_DIR"] = str(SYSROOT)
    e["PKG_CONFIG_ALLOW_SYSTEM_CFLAGS"] = "1"
    e["PKG_CONFIG_ALLOW_SYSTEM_LIBS"] = "1"
    e["LD_LIBRARY_PATH"] = str(SYSLIB)
    e["RUSTUP_HOME"] = "/opt/data/.rustup"
    e["CARGO_HOME"] = "/opt/data/.cargo"
    e["PATH"] = f"{ROOT}/bin:{SYSROOT}/usr/bin:" + e["PATH"]
    # appimagetool shells out to `file`; point its magic db at the sysroot copy
    e["MAGIC"] = str(SYSROOT / "usr/share/misc/magic.mgc")
    e.pop("RUSTFLAGS", None)
    return e


def run(cmd, **kw):
    print(f"+ {' '.join(str(c) for c in cmd)}", flush=True)
    subprocess.run([str(c) for c in cmd], check=True, env=env(), **kw)


def build():
    run(["trunk", "build", "--release", "--config", "frontend/Trunk.toml",
         "index.html"], cwd=ROOT)
    run(["cargo", "build", "--release"], cwd=ROOT / "src-tauri")


def ldd_closure(binaries):
    """Map soname -> resolved path for every lib these binaries need."""
    libs = {}
    e = env()
    todo = list(binaries)
    seen_bins = set()
    while todo:
        b = todo.pop()
        if b in seen_bins:
            continue
        seen_bins.add(b)
        out = subprocess.run(["ldd", str(b)], capture_output=True, text=True,
                             env=e).stdout
        for line in out.splitlines():
            m = re.match(r"\s*(\S+) => (\S+)", line) or re.match(
                r"\s*(\S+) \((0x[0-9a-f]+)\)", line)
            if not m:
                m2 = re.match(r"\s*(\S+) => not found", line)
                if m2:
                    print(f"  WARN: {m2.group(1)} not found (needed by {b.name})")
                continue
            name, path = m.group(1), m.group(2)
            if name in EXCLUDE_LIBS or path.startswith("("):
                continue
            p = Path(path)
            if not p.exists():
                continue
            if name not in libs:
                libs[name] = p
                # a lib may itself need more libs (plugins etc.)
                if p.is_file() and os.access(p, os.X_OK):
                    todo.append(p)
    return libs


def copy_with_links(src: Path, dst_dir: Path):
    """Copy a lib and its symlink chain into dst_dir."""
    src = Path(src)
    real = src.resolve()
    if not (dst_dir / real.name).exists():
        shutil.copy2(real, dst_dir / real.name)
    # recreate each symlink step
    cur = src
    while cur.is_symlink():
        target = os.readlink(cur)
        link = dst_dir / cur.name
        if not link.exists():
            os.symlink(target, link)
        cur = cur.parent / target


def assemble_appdir():
    if APPDIR.exists():
        shutil.rmtree(APPDIR)
    (APPDIR / "usr/bin").mkdir(parents=True)
    (APPDIR / "usr/lib").mkdir(parents=True)
    (APPDIR / "usr/share/glib-2.0/schemas").mkdir(parents=True)

    binary = ROOT / "target/release/rcord"
    if not binary.exists():
        sys.exit("release binary missing — build step failed?")
    shutil.copy2(binary, APPDIR / "usr/bin/rcord")

    # WebKit helper processes + injected bundle (compiled-in libexec path
    # is absolute, so AppRun points WEBKIT_EXEC_PATH at the bundled copy)
    wk_src = SYSLIB / "webkit2gtk-4.1"
    wk_dst = APPDIR / "usr/lib/webkit2gtk-4.1"
    if wk_src.exists():
        shutil.copytree(wk_src, wk_dst, symlinks=True)

    # gio modules (TLS, proxy) and gdk-pixbuf loaders
    for moddir in ("gio/modules", "gdk-pixbuf-2.0"):
        src = SYSLIB / moddir
        if src.exists():
            shutil.copytree(src, APPDIR / "usr/lib" / moddir, symlinks=True)

    # shared-mime-info + glib schemas (GTK dialogs need them)
    schema_src = SYSROOT / "usr/share/glib-2.0/schemas"
    if schema_src.exists():
        shutil.copytree(schema_src, APPDIR / "usr/share/glib-2.0/schemas",
                        dirs_exist_ok=True)

    # library closure of binary + webkit helpers
    helpers = list(wk_dst.glob("*")) if wk_dst.exists() else []
    helpers = [h for h in helpers if h.is_file() and os.access(h, os.X_OK)]
    libs = ldd_closure([binary] + helpers)
    print(f"  bundling {len(libs)} libraries")
    for name, path in sorted(libs.items()):
        copy_with_links(path, APPDIR / "usr/lib")

    # AppRun
    (APPDIR / "AppRun").write_text(
        "#!/bin/sh\n"
        "HERE=\"$(dirname \"$(readlink -f \"$0\")\")\"\n"
        "export LD_LIBRARY_PATH=\"$HERE/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}\"\n"
        "export WEBKIT_EXEC_PATH=\"$HERE/usr/lib/webkit2gtk-4.1\"\n"
        "export GDK_PIXBUF_MODULEDIR=\"$HERE/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders\"\n"
        "export GIO_MODULE_DIR=\"$HERE/usr/lib/gio/modules\"\n"
        "export GSETTINGS_SCHEMA_DIR=\"$HERE/usr/share/glib-2.0/schemas\"\n"
        "exec \"$HERE/usr/bin/rcord\" \"$@\"\n"
    )
    os.chmod(APPDIR / "AppRun", 0o755)

    # .desktop + icon (AppImage convention: root of AppDir)
    (APPDIR / f"{APP_ID}.desktop").write_text(
        "[Desktop Entry]\n"
        "Type=Application\n"
        f"Name={APP_NAME}\n"
        "Comment=Music scale practice recorder\n"
        f"Exec={APP_ID}\n"
        f"Icon={APP_ID}\n"
        "Categories=Audio;AudioVideo;\n"
        "Terminal=false\n"
    )
    shutil.copy2(ROOT / "src-tauri/icons/icon.png", APPDIR / f"{APP_ID}.png")
    hicolor = APPDIR / "usr/share/icons/hicolor/512x512/apps"
    hicolor.mkdir(parents=True)
    shutil.copy2(ROOT / "src-tauri/icons/icon.png", hicolor / f"{APP_ID}.png")


def fetch_appimagetool():
    if APPIMAGE_TOOL.exists():
        return
    print(f"+ download {APPIMAGE_TOOL_URL}")
    urllib.request.urlretrieve(APPIMAGE_TOOL_URL, APPIMAGE_TOOL)
    os.chmod(APPIMAGE_TOOL, 0o755)


def pack_appimage():
    fetch_appimagetool()
    out = DIST / f"{APP_NAME}-x86_64.AppImage"
    if out.exists():
        out.unlink()
    e = env()
    e["ARCH"] = "x86_64"
    # appimagetool is itself an AppImage; extract-and-run avoids FUSE
    print(f"+ {APPIMAGE_TOOL} {APPDIR} {out}")
    subprocess.run(
        [str(APPIMAGE_TOOL), "--appimage-extract-and-run",
         str(APPDIR), str(out)],
        check=True, env=e)
    print(f"AppImage: {out} ({out.stat().st_size / 1e6:.0f} MB)")
    return out


def install_desktop(appimage: Path):
    apps = Path.home() / "Apps"
    apps.mkdir(exist_ok=True)
    dest = apps / f"{APP_NAME}.AppImage"
    shutil.copy2(appimage, dest)
    os.chmod(dest, 0o755)

    icons = Path.home() / ".local/share/icons/hicolor/512x512/apps"
    icons.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "src-tauri/icons/icon.png", icons / f"{APP_ID}.png")

    appsdir = Path.home() / ".local/share/applications"
    appsdir.mkdir(parents=True, exist_ok=True)
    (appsdir / f"{APP_ID}.desktop").write_text(
        "[Desktop Entry]\n"
        "Type=Application\n"
        f"Name={APP_NAME}\n"
        "Comment=Music scale practice recorder\n"
        f"Exec={dest}\n"
        f"Icon={APP_ID}\n"
        "Categories=Audio;AudioVideo;\n"
        "Terminal=false\n"
    )
    udd = shutil.which("update-desktop-database")
    if udd:
        subprocess.run([udd, str(appsdir)], check=False)
    print(f"Installed: {dest}")
    print(f"Launcher:  {appsdir / (APP_ID + '.desktop')}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-desktop-install", action="store_true")
    args = ap.parse_args()
    DIST.mkdir(exist_ok=True)
    build()
    assemble_appdir()
    appimage = pack_appimage()
    if not args.skip_desktop_install:
        install_desktop(appimage)
    print("done.")


if __name__ == "__main__":
    main()
