# Download base image ubuntu 18.04
FROM ubuntu:18.04

ENV DEBIAN_FRONTEND=noninteractive

# Update Software repository
RUN apt-get -qq update

# Install required packets from ubuntu repository
RUN apt-get install -y apt-transport-https curl wget vim nano git binutils autoconf automake make cmake qemu-kvm qemu-system-x86 nasm gcc g++ build-essential libtool bsdmainutils
RUN apt-get install -y libssl-dev pkg-config zlib1g-dev

# add path to hermitcore packets
RUN echo "deb [trusted=yes] https://dl.bintray.com/hermitcore/ubuntu bionic main" | tee -a /etc/apt/sources.list

# Update Software repository
RUN apt-get -qq update

# Install required packets from ubuntu repository
RUN apt-get install -y --allow-unauthenticated binutils-hermit newlib-hermit-rs pte-hermit-rs gcc-hermit-rs libhermit-rs libomp-hermit-rs

# Install Rust toolchain
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain nightly
RUN /root/.cargo/bin/cargo install cargo-xbuild
RUN /root/.cargo/bin/rustup component add rust-src
RUN /root/.cargo/bin/cargo install --git https://github.com/hermitcore/objmv.git
RUN /root/.cargo/bin/cargo install --git https://github.com/hermitcore/pci_ids_parser.git
RUN /root/.cargo/bin/cargo install cargo-tarpaulin

ENV PATH="/opt/hermit/bin:/root/.cargo/bin:${PATH}"
ENV EDITOR=vim
