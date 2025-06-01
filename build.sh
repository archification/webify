#!/bin/bash

#RUSTFLAGS="-Zlocation-detail=none" cargo build --release
RUSTFLAGS="-Zlocation-detail=none" cargo build --target=x86_64-unknown-linux-musl --release
#RUSTFLAGS="-Zlocation-detail=none" cargo build --target x86_64-pc-windows-gnu --release --verbose
#upx --best --lzma target/release/webify
upx --best --lzma target/x86_64-unknown-linux-musl/release/webify
#upx --best --lzma target/x86_64-pc-windows-gnu/release/webify.exe

rm -r /home/jaster/wut/rs/asdf/*
cp /home/jaster/wut/rs/webify/target/x86_64-unknown-linux-musl/release/webify /home/jaster/wut/rs/asdf
