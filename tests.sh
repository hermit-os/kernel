#!/bin/bash

export DEBIAN_FRONTEND="noninteractive"

# Update Software repository
apt-get clean
apt-get -qq update

export PATH=/opt/hermit/bin:$PATH
export PATH="$HOME/.cargo/bin:$PATH"

cargo --version # dump version of the Rust toolchain
cargo install cargo-xbuild
cargo test --lib
make clean
make all
make release=1 all
make qemu
make release=1 qemu
