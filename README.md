# Rcord

Music scale practice recorder for Linux. Plain WAV files in an Obsidian-style
vault folder — record a song in as many segments as you like, organized in
folders you create and name.

- Rust + Tauri 2, Leptos frontend (no JS toolchain)
- Recording via cpal → 16-bit WAV (hound); playback via rodio
- Vault = a normal directory; recordings are normal files you can move/copy/delete anywhere

## Build & ship (AppImage)

```
python3 setup.py
```

Builds the release binary, packs `package/Rcord-x86_64.AppImage` (GTK/WebKit
runtime bundled), and installs a desktop launcher + icon
(`~/Apps/Rcord.AppImage`, `~/.local/share/applications/rcord.desktop`).
Use `--skip-desktop-install` to only produce the AppImage.

## Dev

```
source env.sh          # pins sysroot, rustup/cargo homes, tool binaries
cd src-tauri && ../bin/cargo-tauri dev
```

Host with no system webkit? The build uses the userland sysroot at
`/opt/data/opt/tauri-sys` (see `env.sh`). On a normal desktop, install
`libwebkit2gtk-4.1-dev libgtk-3-dev libasound2-dev` and skip the sysroot.

## Test

```
source env.sh && cd src-tauri && cargo test
```
