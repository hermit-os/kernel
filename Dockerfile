FROM rwthos/hermit-cargo:latest

ENV DEBIAN_FRONTEND=noninteractive

# Update Software repository
RUN apt-get clean 
RUN apt-get -qq update

RUN PATH="/opt/hermit/bin:/root/.cargo/bin:${PATH}" /root/.cargo/bin/cargo install cargo-xbuild
RUN PATH="/opt/hermit/bin:/root/.cargo/bin:${PATH}" /root/.cargo/bin/cargo install cargo-tarpaulin

# Switch back to dialog for any ad-hoc use of apt-get
ENV DEBIAN_FRONTEND=dialog

