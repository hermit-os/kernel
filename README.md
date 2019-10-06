<img width="100" align="right" src="img/hermitcore_logo.png" />

# RustyHermit - A Rust-based, lightweight unikernel

[![Build Status](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/badges/master/pipeline.svg)](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/pipelines)
[![Build Status](https://travis-ci.org/hermitcore/libhermit-rs.svg?branch=master)](https://travis-ci.org/hermitcore/libhermit-rs)
[![License](https://img.shields.io/crates/l/rusty-hermit.svg)](https://img.shields.io/crates/l/rusty-hermit.svg)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

[HermitCore]( http://www.hermitcore.org ) is a [unikernel](http://unikernel.org) targeting a scalable and predictable runtime for high-performance and cloud computing.
Unikernel means, you bundle your application directly with the kernel library, so that it can run without any installed operating system.
This reduces overhead, therfore, interesting applications include virtual machines and high-performance computing.

The RustyHermit can run Rust applications, as well as C/C++/Go/Fortran applications.
A tutorial on how to use these programming languages on top of RustyHermit is published at [https://github.com/hermitcore/hermit-playground](https://github.com/hermitcore/hermit-playground).

## History/Background

HermitCore is a research project at [RWTH-Aachen](https://www.rwth-aachen.de) and was originally written in C ([libhermit](https://github.com/hermitcore/libhermit)).
We decided to develop a new version of HermitCore in [Rust](https://www.rust-lang.org) and name it **RustyHermit**.
The ownership  model of Rust guarantees memory/thread-safety and enables us to eliminate many classes of bugs at compile-time.
Consequently, the use of Rust for kernel development promises less vulnerabilities in comparsion to common programming languages.

The kernel and the integration into the Rust runtime is entirely written in Rust and does not use any C/C++ Code.
We extend the Rust toolchain so that the build process is similar to Rust's usual workflow.
Rust applications that do not bypass the Rust runtime and directly use OS services are able to run on RustyHermit without modifications.

## Installation

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

## Running RustyHermit

### Using uhyve as hypervisor

RustyHermit can run within our own hypervisor [*uhyve*](https://github.com/hermitcore/uhyve) , which requires [KVM](https://www.linux-kvm.org/) to create a virtual machine.
Please install the hypervisor as follows:

```sh
cargo install uhyve
```

Afterwards, your are able to start RustyHermit applications within our hypervisor:

```sh
uhyve target/x86_64-unknown-hermit/debug/hello_world
```

The maximum amount of memory can be configured via environment variables like in the following example

```sh
HERMIT_CPUS=4 HERMIT_MEM=8G uhyve target/x86_64-unknown-hermit/debug/hello_world
```

The virtual machine is configured using the following environment variables

Variable         | Default     | Description
-----------------|-------------|--------------
`HERMIT_CPUS`    | 1           | Number of cores the virtual machine may use
`HERMIT_MEM`     | 512M        | Memory size of the virtual machine. The suffixes *M* and *G* can be used to specify a value in megabytes or gigabytes
`HERMIT_VERBOSE` | 0           | Hypervisor prints kernel log messages stdout. ("1" enables log)

For instance, the following command starts the demo application in a virtual machine, which has 4 cores and 8GiB memory:

```bash
$ HERMIT_CPUS=4 HERMIT_MEM=8G ./proxy ../../hello_world/target/x86_64-unknown-hermit/debug/hello_world
```

More details can be found in the uhyve README.

### Using Qemu as hypervisor

It is also possible to run RustHermit within [Qemu](https://www.qemu.org).
In this case, a loader is required to boot the application.
This loader is part of the repository and can be build with [xbuid](https://github.com/rust-osdev/cargo-xbuild) as follows.

```bash
$ cd loader
$ cargo xbuild --target x86_64-unknown-hermit-loader.json
```

Afterwards, the loader is stored in `target/x86_64-unknown-hermit-loader/debug/` as `hermit-loader`.
Afterwards, the unikernel application `app` can be booted with following command:

```bash
$ qemu-system-x86_64 -display none -smp 1 -m 64M -serial stdio  -kernel path_to_loader/hermit-loader -initrd path_to_app/app -cpu qemu64,apic,fsgsbase,rdtscp,xsave,fxsr
```

It is important to enable the processor features _fsgsbase_ and _rdtscp_ because it is a prerequisite to boot RustyHermit.

## Use RustyHermit for C/C++, Go, and Fortran applications

This kernel can still be used with C/C++, Go, and Fortran applications.
A tutorial on how to do this is available at [https://github.com/hermitcore/hermit-playground](https://github.com/hermitcore/hermit-playground).

## Missing features

* Multikernel support (might be comming)
* Virtio support (comming soon)
* Network suppot (comming soon)

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
