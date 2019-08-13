#!/bin/bash

OS_NAME=$1
OS_VERSION=$2

export DEBIAN_FRONTEND="noninteractive"

apt-get -qq update || exit 1
apt-get install -y --no-install-recommends binutils bsdmainutils ca-certificates cmake curl gcc git libc-dev make nasm qemu-system-x86 rpm || exit 1
apt-get install -y --no-install-recommends libssl-dev pkg-config cmake zlib1g-dev

echo "deb [trusted=yes] http://dl.bintray.com/hermitcore/ubuntu bionic main" >> /etc/apt/sources.list
apt-get -qq update || exit 1
apt-get install -y --allow-unauthenticated binutils-hermit gcc-hermit-rs libomp-hermit-rs newlib-hermit-rs pte-hermit-rs || exit 1
export PATH=/opt/hermit/bin:$PATH
export PATH="$HOME/.cargo/bin:$PATH"

curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain nightly
cargo --version # dump version of the Rust toolchain
cargo install cargo-xbuild
rustup component add rust-src
cargo test --lib
