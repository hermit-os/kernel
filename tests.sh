#!/bin/bash

export DEBIAN_FRONTEND="noninteractive"

# Update Software repository
apt-get clean
apt-get -qq update

# Install required packets from ubuntu repository
apt-get install -y apt-transport-https curl wget vim nano git binutils autoconf automake make cmake qemu-kvm qemu-system-x86 nasm gcc g++ build-essential libtool bsdmainutils

# Install Rust compiler
curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain nightly

export PATH="$HOME/.cargo/bin:$PATH"

cargo --version # dump version of the Rust toolchain
rustup component add rust-src
rustup component add llvm-tools-preview

cargo build -Z build-std=core,alloc --target x86_64-unknown-hermit-kernel
cargo build -Z build-std=core,alloc --target x86_64-unknown-hermit-kernel --release
cargo test --target x86_64-unknown-linux-gnu