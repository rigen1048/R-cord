# Rcord build environment — source this before building
export PKG_CONFIG_PATH=/opt/data/opt/tauri-sys/usr/lib/x86_64-linux-gnu/pkgconfig:/opt/data/opt/tauri-sys/usr/share/pkgconfig
export PKG_CONFIG_SYSROOT_DIR=/opt/data/opt/tauri-sys
export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1
export PKG_CONFIG_ALLOW_SYSTEM_LIBS=1
export LD_LIBRARY_PATH=/opt/data/opt/tauri-sys/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
export PATH=/opt/data/projects/rcord/bin:$PATH
# rustup homes differ across shells on this host; pin them explicitly
export RUSTUP_HOME=/opt/data/.rustup
export CARGO_HOME=/opt/data/.cargo
unset RUSTFLAGS
