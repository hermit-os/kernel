#!/bin/bash

OS_NAME=$1
OS_VERSION=$2

export DEBIAN_FRONTEND="noninteractive"

apt-get -qq update || exit 1
apt-get install -y --no-install-recommends binutils bsdmainutils ca-certificates cmake curl gcc git libc-dev make nasm qemu-system-x86 rpm || exit 1

echo "deb [trusted=yes] http://dl.bintray.com/hermitcore/ubuntu bionic main" >> /etc/apt/sources.list
apt-get -qq update || exit 1
apt-get install -y --allow-unauthenticated binutils-hermit gcc-hermit-rs libomp-hermit-rs newlib-hermit-rs pte-hermit-rs || exit 1
export PATH=/opt/hermit/bin:$PATH
export PATH="$HOME/.cargo/bin:$PATH"

curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain nightly
cargo --version # dump version of the Rust toolchain
cargo install cargo-xbuild
rustup component add rust-src
mkdir build
cd build
cmake ..
make -j1 package || exit 1

cd ..
mkdir -p tmp
dpkg-deb -R build/libhermit-rs-0.3.5-all.deb tmp || exit 1
rm -rf build/*.deb build/_CPack_Packages

TDIR=/work/build/local_prefix/opt/hermit/x86_64-hermit/extra
FILES="$TDIR/tests/hello $TDIR/tests/hellof $TDIR/tests/hello++ $TDIR/tests/thr_hello $TDIR/benchmarks/stream $TDIR/tests/test-malloc"
PROXY=/work/build/local_prefix/opt/hermit/bin/proxy

for f in $FILES; do echo "check $f..."; HERMIT_ISLE=qemu HERMIT_CPUS=1 HERMIT_KVM=0 HERMIT_VERBOSE=1 timeout --kill-after=5m 5m $PROXY $f || exit 1; done

for f in $FILES; do echo "check $f..."; HERMIT_ISLE=qemu HERMIT_CPUS=2 HERMIT_KVM=0 HERMIT_VERBOSE=1 timeout --kill-after=5m 5m $PROXY $f || exit 1; done

# test echo server at port 8000
#HERMIT_ISLE=qemu HERMIT_CPUS=1 HERMIT_KVM=0 HERMIT_VERBOSE=1 HERMIT_APP_PORT=8000 $PROXY $TDIR/tests/server &
#sleep 10
#curl http://127.0.0.1:8000/help
#sleep 1
#curl http://127.0.0.1:8000/hello
##sleep 1

# kill server
#kill $!

# test connection via netio
#wget http://web.ars.de/wp-content/uploads/2017/04/netio132.zip
#unzip netio132.zip
#HERMIT_ISLE=qemu HERMIT_CPUS=2 HERMIT_KVM=0 HERMIT_VERBOSE=1 HERMIT_APP_PORT=18767 $PROXY $TDIR/benchmarks/netio &
#sleep 1
#chmod a+rx bin/linux-x86_64
#bin/linux-x86_64 -t -b 4k localhost
#sleep 1

# kill server
#kill $!
