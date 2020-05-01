<img width="100" align="right" src="img/hermitcore_logo.png" />

# RustyHermit - A Rust-based, lightweight unikernel

[![Build Status](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/badges/master/pipeline.svg)](https://git.rwth-aachen.de/acs/public/hermitcore/libhermit-rs/pipelines)
![Actions Status](https://github.com/hermitcore/libhermit-rs/workflows/Build/badge.svg)
[![License](https://img.shields.io/crates/l/rusty-hermit.svg)](https://img.shields.io/crates/l/rusty-hermit.svg)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

[HermitCore]( http://www.hermitcore.org ) is a [unikernel](http://unikernel.org) targeting a scalable and predictable runtime for high-performance and cloud computing.
Unikernel means, you bundle your application directly with the kernel library, so that it can run without any installed operating system.
This reduces overhead, therefore, interesting applications include virtual machines and high-performance computing.

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

## Building RusytHermit

It is required to install the Rust toolchain.
Please visit the [Rust website](https://www.rust-lang.org/) and follow the installation instructions for your operating system.
It is important that the **nightly channel** is used to install the toolchain.
```sh
rustup default nightly
```

After the installation of the toolchain, the source code of the Rust runtime, the cargo subcommand [cargo-download](https://crates.io/crates/cargo-download), and llvm-tools have to be installed as follow:

```sh
cargo install cargo-download
rustup component add rust-src
rustup component add llvm-tools-preview
```

As an example, the following commands create a template for the test application *Hello World*.

```sh
cargo new hello_world --bin
cd hello_world
```

To bind the library operating system to the application, add in the file *Cargo.toml* the crate [hermit-sys](https://crates.io/crates/hermit-sys) to the list of dependency.
In addition, it is important to use at least the optimization level 1.
Consequently, it is required to **extend** *Cargo.toml* with following lines.

```toml
[target.'cfg(target_os = "hermit")'.dependencies]
hermit-sys = "0.1.*"

[profile.release]
opt-level = 3
debug = false
rpath = false
lto = true
debug-assertions = false

[profile.dev]
opt-level = 1
debug = true
rpath = false
lto = false
debug-assertions = true
```

Finally, import the crate in the main file of your application.

```rust,no_run
#![allow(unused_imports)]

#[cfg(target_os = "hermit")]
extern crate hermit_sys;

fn main() {
        println!("Hello World!");
}
```

The final step is building the application as follows:

```sh
cargo build -Z build-std=std,core,alloc,panic_abort --target x86_64-unknown-hermit
```

If the command failed with the error message

```sh
linker `rust-lld` not found
```

the path to the *llvm-tools* is not set.
On Linux, it is typically installed at *${HOME}/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin*.
```sh
PATH=${HOME}/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin:$PATH cargo build -Z build-std=std,core,alloc,panic_abort --target x86_64-unknown-hermit
```
Otherwise, the linker can be replaced by *lld* as follows:

```sh
RUSTFLAGS="-C linker=lld" cargo build -Z build-std=std,core,alloc,panic_abort --target x86_64-unknown-hermit
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
$ HERMIT_CPUS=4 HERMIT_MEM=8G uhyve target/x86_64-unknown-hermit/debug/hello_world
```

More details can be found in the uhyve README.

### Using Qemu as hypervisor

It is also possible to run RustyHermit within [Qemu](https://www.qemu.org).
In this case, the loader [rusty-loader](https://github.com/hermitcore/rusty-loader) is required to boot the application.
To build the loader, the cargo subcommand [xbuild](https://github.com/rust-osdev/cargo-xbuild) and the assembler [nasm](https://www.nasm.us) are required.
After the installation of [xbuild](https://github.com/rust-osdev/cargo-xbuild), the loader can be build as follows.

```bash
$ git clone https://github.com/hermitcore/rusty-loader.git
$ cd rusty-loader
$ make
```

Afterwards, the loader is stored in `target/x86_64-unknown-hermit-loader/debug/` as `rusty-loader`.
As final step, the unikernel application `app` can be booted with following command:

```bash
$ qemu-system-x86_64 -display none -smp 1 -m 64M -serial stdio  -kernel path_to_loader/rusty-loader -initrd path_to_app/app -cpu qemu64,apic,fsgsbase,rdtscp,xsave,fxsr
```

It is important to enable the processor features _fsgsbase_ and _rdtscp_ because it is a prerequisite to boot RustyHermit.

### Using virtio-fs

The Kernel has rudimentary support for the virtio-fs shared file system. Currently only files, no folders are supported. To use it, you have to run a virtio-fs daemon and start qemu as described in [Standalone virtio-fs usage](https://virtio-fs.gitlab.io/howto-qemu.html):

```bash
# start virtiofsd in the background
$ sudo virtiofsd --thread-pool-size=1 --socket-path=/tmp/vhostqemu -o source=$(pwd)/SHARED_DIRECTORY
# give non-root-users access to the socket
$ sudo chmod 777 /tmp/vhostqemu
# start qemu with virtio-fs device.
# you might want to change the socket (/tmp/vhostqemu) and virtiofs tag (currently myfs)
$ qemu-system-x86_64 -cpu qemu64,apic,fsgsbase,rdtscp,xsave,fxsr -enable-kvm -display none -smp 1 -m 1G -serial stdio \
        -kernel path_to_loader/rusty-loader \
        -initrd path_to_app/app \
        -chardev socket,id=char0,path=/tmp/vhostqemu \
        -device vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=myfs \
        -object memory-backend-file,id=mem,size=1G,mem-path=/dev/shm,share=on \
        -numa node,memdev=mem
```

You can now access the files in SHARED_DIRECTORY under the virtiofs tag like `/myfs/testfile`.

## Extending RustyHermit

The best way to extend the kernel is to work with the branch *devel* of the repository [rusty-hermit](https://github.com/hermitcore/rusty-hermit).
It includes this repository as submodule and link the unikernel directly to the test application.

According to the following instructions, the test application can be found under *target/x86_64-unknown-hermit/debug/rusty_demo*.

```sh
git clone https://github.com/hermitcore/rusty-hermit.git
cd rusty-hermit
git submodule init
git submodule update
cargo build -Z build-std=std,core,alloc,panic_abort --target x86_64-unknown-hermit
```

## Use RustyHermit for C/C++, Go, and Fortran applications

This kernel can still be used with C/C++, Go, and Fortran applications.
A tutorial on how to do this is available at [https://github.com/hermitcore/hermit-playground](https://github.com/hermitcore/hermit-playground).

## Missing features

* Multikernel support (might be comming)
* Virtio support (comming soon)
* Network support (comming soon)

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
