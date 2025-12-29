#!/bin/bash

#export OPENSSL_DIR=$(dirname $(dirname $(which openssl)))
#export PKG_CONFIG_PATH=/usr/lib/pkgconfig

#cargo install cross --git https://github.com/cross-rs/cross

# linux
#RUSTFLAGS="-Zlocation-detail=none" cargo build --release
RUSTFLAGS="-Zlocation-detail=none" cross build --target x86_64-unknown-linux-musl --release
# windows
#RUSTFLAGS="-Zlocation-detail=none" cargo build --target x86_64-pc-windows-gnu --release --verbose
#RUSTFLAGS="-Zlocation-detail=none" cross build --target x86_64-pc-windows-gnu --release --verbose
cargo xwin build --release --target x86_64-pc-windows-msvc

#upx --best --lzma target/release/webify
upx --best --lzma target/x86_64-unknown-linux-musl/release/webify
#strip target/x86_64-pc-windows-gnu/release/webify.exe
#upx --best --lzma target/x86_64-pc-windows-gnu/release/webify.exe
strip target/x86_64-pc-windows-msvc/release/webify.exe
upx --best --lzma target/x86_64-pc-windows-msvc/release/webify.exe
