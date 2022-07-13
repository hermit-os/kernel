# Derived from rust:bullseye
FROM buildpack-deps:bullseye as hermit-toolchain

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    # Manually sync this with rust-toolchain.toml!
    RUST_VERSION=nightly-2022-06-29 \
    RUST_COMPONENTS="clippy llvm-tools-preview rustfmt rust-src" \
    RUST_TARGETS="x86_64-unknown-none"

RUN set -eux; \
    dpkgArch="$(dpkg --print-architecture)"; \
    case $dpkgArch in \
        amd64) rustArch='x86_64-unknown-linux-gnu'; rustupSha256='3dc5ef50861ee18657f9db2eeb7392f9c2a6c95c90ab41e45ab4ca71476b4338' ;; \
        armhf) rustArch='armv7-unknown-linux-gnueabihf'; rustupSha256='67777ac3bc17277102f2ed73fd5f14c51f4ca5963adadf7f174adf4ebc38747b' ;; \
        arm64) rustArch='aarch64-unknown-linux-gnu'; rustupSha256='32a1532f7cef072a667bac53f1a5542c99666c4071af0c9549795bbdb2069ec1' ;; \
        i386) rustArch='i686-unknown-linux-gnu'; rustupSha256='e50d1deb99048bc5782a0200aa33e4eea70747d49dffdc9d06812fd22a372515' ;; \
        *) echo >&2 "unsupported architecture: $dpkgArch"; exit 1 ;; \
    esac; \
    url="https://static.rust-lang.org/rustup/archive/1.24.3/$rustArch/rustup-init"; \
    wget "$url"; \
    echo "$rustupSha256 *rustup-init" | sha256sum -c -; \
    chmod +x rustup-init; \
    ./rustup-init -y --no-modify-path \
        --profile minimal \
        --default-toolchain $RUST_VERSION \
        --default-host $rustArch \
        --component $RUST_COMPONENTS \
        --target $RUST_TARGETS \
    ; \
    rm rustup-init; \
    chmod -R a+w $RUSTUP_HOME $CARGO_HOME; \
    rustup --version; \
    cargo --version; \
    rustc --version;

# Build dependencies with stable toolchain channel
FROM rust:bullseye as stable-deps
RUN set -eux; \
    cargo install uhyve;

# Build dependencies with libhermit-rs' toolchain channel
FROM hermit-toolchain as hermit-deps
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        nasm \
    ; \
    rm -rf /var/lib/apt/lists/*; \
    git clone https://github.com/hermitcore/rusty-loader.git; \
    cd rusty-loader; \
    cargo xtask build --arch x86_64 --release;

# Install dependencies
FROM hermit-toolchain as ci-runner
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        nasm \
        # For kvm-ok:
        cpu-checker \
        qemu-system-x86 \
    ; \
    rm -rf /var/lib/apt/lists/*;
COPY --from=stable-deps $CARGO_HOME/bin/uhyve $CARGO_HOME/bin/uhyve
COPY --from=hermit-deps rusty-loader/target/x86_64/release/rusty-loader /usr/local/bin/rusty-loader
