<img width="100" align="right" src="img/hermitcore_logo.png" />

# RustyHermit - A Rust-based, lightweight unikernel

[![Build Status](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/badges/master/pipeline.svg)](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/pipelines)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

[HermitCore]( http://www.hermitcore.org ) is a
[unikernel](http://unikernel.org) targeting a scalable and predictable runtime for high-performance and cloud computing.

We decided to develop a new version of HermitCore in [Rust](https://www.rust-lang.org) and call it **RustyHermit**.
Rust guarantees memory/thread-safety and prevents various common bugs at compile-time.
Consequently, the use of Rust for kernel development promises less vulnerabilities in comparsion to other common programming languages.

The kernel and the integration into the Rust runtime is entirely written in Rust and does not use any C/C++.
We extend the Rust toolchain so that the build process is similar to Rust's usual workflow.
Rust applications that do not bypass the Rust runtime and directly use OS services are able to run on RustyHermit without modifications.

## How to use RustyHermit for pure Rust applications

### Compilation

We provide a Docker container *hermitcore-rs* for easy compilation of Rust applications into a unikernel.
Please pull the container and use *cargo* to cross compile the application.
As an example, the following commands create the test application *Hello World* for RustyHermit.

```sh
docker pull rwthos/hermitcore-rs
docker run -v $PWD:/volume -e USER=$USER --rm -t rwthos/hermitcore-rs cargo new hello_world --bin
cd hello_world
docker run -v $PWD:/volume -e USER=$USER --rm -t rwthos/hermitcore-rs cargo build --target x86_64-unknown-hermit
cd -
```

### Run

RustyHermit must be run within our own hypervisor *uhyve*, which uses [KVM](https://www.linux-kvm.org/) to create a virtual machine.
Please follow the README of the [hermitcave repository](https://github.com/hermitcore/hermit-caves/tree/path2rs).
Following the README will create the *proxy*, that can be used to start RustyHermit applications:

```sh
./proxy ../../hello_world/target/x86_64-unknown-hermit/debug/hello_world
```

The maximum amount of memory can be configured via environment variables like in the following example

```sh
HERMIT_CPUS=4 HERMIT_MEM=8G ./proxy ../../hello_world/target/x86_64-unknown-hermit/debug/hello_world
```

More details can be found in the uhyve README.

## Use RustyHermit for C/C++, Go, and Fortran applications

This kernel can still be used with C/C++, Go, and Fortran applications.
A tutorial on how to do this is available at [https://github.com/hermitcore/hermit-playground](https://github.com/hermitcore/hermit-playground).

## Missing features

In contrast to the C version of HermitCore, RustyHermit is currently not able to run as multikernel.
In addition, running the applications baremetal, i.e., directly on the hardware or within other hypervisors is currently not fully supported, but will be added at a later date.

## Credits

RustyHermit is derived from following tutorials and software distributions:

1. Philipp Oppermann's [excellent series of blog posts][opp].
2. Erik Kidd's [toyos-rs][kidd], which is an extension of Philipp Opermann's kernel.
3. The Rust-based teaching operating system [eduOS-rs][eduos].

[opp]: http://blog.phil-opp.com/
[kidd]: http://www.randomhacks.net/bare-metal-rust/
[eduos]: http://rwth-os.github.io/eduOS-rs/

HermitCore's Emoji is provided for free by [EmojiOne](https://www.gfxmag.com/crab-emoji-vector-icon/).

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

RustyHermit is being developed on [GitHub](https://github.com/hermitcore/libhermit-rs).
Create your own fork, send us a pull request, and chat with us on [Slack](https://radiant-ridge-95061.herokuapp.com)
