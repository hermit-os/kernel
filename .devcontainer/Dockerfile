FROM hermitcore/rustyhermit:latest

ENV DEBIAN_FRONTEND=noninteractive

# Update Software repository
RUN apt-get clean 
RUN apt-get -qq update

RUN apt-get install net-tools

#RUN PATH="/opt/hermit/bin:/root/.cargo/bin:${PATH}" /root/.cargo/bin/rustup toolchain link rusty /usr/local/
#RUN PATH="/opt/hermit/bin:/root/.cargo/bin:${PATH}" /root/.cargo/bin/rustup default rusty
#RUN PATH="/opt/hermit/bin:/root/.cargo/bin:${PATH}" /root/.cargo/bin/cargo install cargo-xbuild
#RUN PATH="/opt/hermit/bin:/root/.cargo/bin:${PATH}" /root/.cargo/bin/cargo install --git https://github.com/hermitcore/bootimage.git --branch hermit bootimage

# Switch back to dialog for any ad-hoc use of apt-get
ENV DEBIAN_FRONTEND=dialog

