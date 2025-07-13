#!/bin/bash

export OPENSSL_DIR=$(dirname $(dirname $(which openssl)))
export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig

#RUSTFLAGS="-Zlocation-detail=none" cargo build --release
cargo build --target=x86_64-unknown-linux-musl --release
#RUSTFLAGS="-Zlocation-detail=none" cargo build --target x86_64-pc-windows-gnu --release --verbose
#upx --best --lzma target/release/webify
upx --best --lzma target/x86_64-unknown-linux-musl/release/webify
#upx --best --lzma target/x86_64-pc-windows-gnu/release/webify.exe
