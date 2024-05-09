#!/bin/bash

RUSTFLAGS="-Zlocation-detail=none" cargo build --release
RUSTFLAGS="-Zlocation-detail=none" cargo build --target=x86_64-unknown-linux-musl --release
RUSTFLAGS="-Zlocation-detail=none" cargo build --target x86_64-pc-windows-gnu --release --verbose
#RUSTFLAGS="-Zlocation-detail=none" cargo build --target x86_64-apple-darwin --release --verbose
#RUSTFLAGS="-Zlocation-detail=none" cargo build --target aarch64-apple-darwin --release --verbose
upx --best --lzma target/release/webify
upx --best --lzma target/x86_64-unknown-linux-musl/release/webify
upx --best --lzma target/x86_64-pc-windows-gnu/release/webify.exe
#upx --best --lzma target/x86_64-apple-darwin/release/webify
#upx --best --lzma target/aarch64-apple-darwin/release/webify
