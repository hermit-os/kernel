<img width="100" align="right" src="img/hermitcore_logo.png" />


# RustyHermit - A Rust-based, lightweight unikernel

[![Build Status](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/badges/master/pipeline.svg)](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/pipelines)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

[HermitCore]( http://www.hermitcore.org ) is a
[unikernel](http://unikernel.org) targeting a scalable and predictable runtime for high-performance and cloud computing.

We decided to develop a new version of HermitCore in [Rust](https://www.rust-lang.org) and called it **HermitCore-rs** also known as **RustyHermit**.
Rust guarantees memory/thread-safety by its ownership model and enables to eliminate many classes of bugs at compile-time.
Consequently, the usage of Rust for kernel development promises less vulnerabilities in comparsion to common programming languages.

The kernel still supports the development of C/C++/Go/Fortran applications.
A tutorial to use these programming languages on top of RustyHermit is published at [https://github.com/hermitcore/hermit-playground](https://github.com/hermitcore/hermit-playground).

This repository shows, how to build pure Rust applications on top of RustyHermit.
The kernel and the integration into Rust's runtime is completly written in Rust and does not use C/C++.
We extend the Rust toolchain so that the build process is similar to Rust's common workflow.
Rust applications, which do not bypass the Rust runtime and directly use OS services are able to run on RustyHermit without modification.

Currently, the easiest way to compile a Rust application into a unikernel is the usage of the Docker container *hermitcore-rs*.
Please pull the container and use *cargo* to cross compile the application.
For instance, the following commands create the test application *Hello World* for RustyHermit.

```sh
$ docker pull rwthos/hermitcore-rs
$ docker run -v $PWD:/volume -e USER=$USER --rm -t rwthos/hermitcore-rs cargo new hello_world --bin
$ cd hello_world
$ docker run -v $PWD:/volume -e USER=$USER --rm -t rwthos/hermitcore-rs cargo build --target x86_64-unknown-hermit
$ cd -
```

Currently, the unikernel can only run within our own hypervisor *uhyve*, which requires [KVM](https://www.linux-kvm.org/) to create a virtual machine.
To build *uhyve* you need following tools:

* x86-based Linux systems
* Recent host compiler such as GCC
* CMake	
* git

As a first step to build the hypervisor, its repository has to be cloned:

```sh
$ git clone -b path2rs https://github.com/hermitcore/hermit-caves.git
```

To build the hypervisor, go to the directory with the source code and use the following commands:

```sh
$ cd hermit-caves
$ mkdir build
$ cd build
$ cmake ..
$ make
```

Afterwards, you find in the working directory the application *proxy*.
Use this application to start the unikernel.

```sh
$ ./proxy ../../hello_world/target/x86_64-unknown-hermit/debug/hello_world
```

The environment variable `HERMIT_CPUS` specifies the number of
CPUs.
Furthermore, the variable `HERMIT_MEM` defines the memory size of the virtual machine. The suffixes *M* and *G* can be used to specify a value in megabytes or gigabytes respectively.
By default, the loader initializes a system with one core and 512 MiB RAM.
For instance, the following command starts the demo application in a virtual machine, which has 4 cores and 8GiB memory:

```bash
$ HERMIT_CPUS=4 HERMIT_MEM=8G ./proxy ../../hello_world/target/x86_64-unknown-hermit/debug/hello_world
```

By setting the environment variable `HERMIT_VERBOSE` to `1`, the hypervisor prints
also the kernel log messages to the screen.

```bash
$ HERMIT_VERBOSE=1 ./proxy ../../hello_world/target/x86_64-unknown-hermit/debug/hello_world
```

## Missing features

In contrast to the C version of HermitCore, RustyHermit is currently not able to run as multikernel.
In addition, running the applications baremetal on the hardware or within common hyperisors is currently limited and added at a later date.

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

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

RustyHermit is being developed on [GitHub](https://github.com/hermitcore/libhermit-rs).
Create your own fork, send us a pull request, and chat with us on [Slack](https://radiant-ridge-95061.herokuapp.com)
