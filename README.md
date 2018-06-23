<img width="100" align="right" src="img/hermitcore_logo.png" />


# HermitCore - A Rust-based, lightweight unikernel for a scalable and predictable runtime behavior

[![Build Status](https://travis-ci.org/hermitcore/libhermit-rs.svg?branch=master)](https://travis-ci.org/hermitcore/libhermit)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

The project [HermitCore]( http://www.hermitcore.org ) is a new
[unikernel](http://unikernel.org) targeting a scalable and predictable runtime
for high-performance and cloud computing. HermitCore extends the multi-kernel
approach (like
[McKernel](https://www-sys-aics.riken.jp/ResearchTopics/os/mckernel/)) with
unikernel features for a better programmability and scalability for hierarchical
systems.

__We decided to develop a version of the kernel in [Rust](https://www.rust-lang.org).
We promise that this will make it easier to maintain and to extend our kernel.
All code beside the kernel will be still developed in their preferred language (C/C++/Go/Fortran).__

__Consequently, this branch represents the transition from C to Rust.
Currently, the Rust-based version supports not all features of the [C-based version](https://github.com/hermitcore/libhermit).
However, it is a starting point and runs within a hypervisor.
The multi-kernel approach is currently in the Rust-based version not yet fully
supported and under development.__

## Contributing

HermitCore is being developed on [GitHub](https://github.com/hermitcore/libhermit-rs).
Create your own fork, send us a pull request, and chat with us on [Slack](https://radiant-ridge-95061.herokuapp.com).

## Requirements

The build process works currently only on **x86-based Linux** systems. To build
the HermitCore kernel and applications you need:

 * CMake
 * Netwide Assember (NASM)
 * recent host compiler such as GCC
 * HermitCore cross-toolchain, i.e. Binutils, GCC, newlib, pthreads
 * [Rust compiler (nightly release)](https://www.rust-lang.org/en-US/install.html)
 * Install [xargo](https://github.com/japaric/xargo) with `cargo install xargo`
 * Xargo depends on the rust source code, which we can install with `rustup component add rust-src`.

### HermitCore cross-toolchain

We provide prebuilt packages (currently Ubuntu 18.04 only) of the HermitCore
toolchain. The packages based on the C version branch and can be installed as follows:

```bash
$ echo "deb [trusted=yes] https://dl.bintray.com/hermitcore/ubuntu bionic main" | sudo tee -a /etc/apt/sources.list
$ sudo apt-get -qq update
$ sudo apt-get install binutils-hermit newlib-hermit pte-hermit-rs gcc-hermit libhermit-rs
```

If you want to build the toolchain yourself, have a look at the repository [hermit-toolchain](https://github.com/hermitcore/hermit-toolchain), which contains in the branch `path2rs` scripts to build the whole toolchain for the Rust-based version of HermitCore.

Depending on how you want to use HermitCore, you might need additional packages
such as:

 * QEMU (`apt-get install qemu-system-x86`)

## Building the Rust-based Kernel

### Preliminary work

To use the Rust-based version, HermitCore has to build from source. For this
purpose, the repository with its submodules has to be clone.

```bash
$ git clone git@github.com:hermitcore/libhermit-rs.git
$ cd libhermit-rs
$ git submodule init
$ git submodule update
```

### Building the library operating systems and its examples

To build Rust-based kernel and its examples, go to the directory with the source code,
create a subdirectory `build`, and call in the new directory `cmake` followed by `make`.

```bash
$ mkdir build
$ cd build
$ cmake ..
$ make
$ sudo make install
```

If your toolchain is not located in `/opt/hermit/bin` then you have to supply
its location to the `cmake` command above like so:

```bash
$ cmake -DTOOLCHAIN_BIN_DIR=/home/user/hermit/bin ..
```

Assuming that binaries like `x86_64-hermit-gcc` and friends are located in that
directory.
To install your new version in the same directory, you have to set the installation path and to install HermitCore as follows:

```bash
$ cmake -DTOOLCHAIN_BIN_DIR=/home/user/hermit/bin -DCMAKE_INSTALL_PREFIX=/home/user/hermit ..
$ make
$ make install
```

**Note:** If you use the cross compiler outside of this repository, the compiler uses per default the library operating systems located by the toolchain (e.g. `/opt/hermit/x86_64-hermit/lib/libhermit.a`).

## Proxy

Part of HermitCore is a small helper tool, which is called *proxy*.
This tool helps to start HermitCore applications within a virtual machine or bare-metal on a NUMA node.
In principle it is a bridge to the Linux system.
If the proxy is register as loader to the Linux system, HermitCore applications can be started like common Linux applications.
The proxy can be registered with following command.

```bash
$ sudo -c sh 'echo ":hermit:M:7:\\x42::/opt/hermit/bin/proxy:" > /proc/sys/fs/binfmt_misc/register'
$ # dirct call of a HermitCore appliaction
$ /opt/hermit/x86_64-hermit/extra/tests/hello
```

Otherwise the proxy must be started directly and get the path to HermitCore application as argument.
Afterwards, the proxy start the HermitCore applications within a VM ore bare-metal on a NUMA node.

```bash
$ # using QEMU
$ HERMIT_ISLE=qemu /opt/hermit/bin/proxy /opt/hermit/x86_64-hermit/extra/tests/hello
```

## Testing

### As classical standalone unikernel within a virtual machine

HermitCore applications can be directly started as standalone kernel within a
virtual machine.

```bash
$ cd build
$ make install DESTDIR=~/hermit-build
$ cd ~/hermit-build/opt/hermit
$ # using QEMU
$ HERMIT_ISLE=qemu bin/proxy x86_64-hermit/extra/tests/hello
$ # using uHyve
$ HERMIT_ISLE=uhyve bin/proxy x86_64-hermit/extra/tests/hello
```

With `HERMIT_ISLE=qemu`, the application will be started within a QEMU VM.
Please note that the loader requires QEMU and uses per default *KVM*.
Furthermore, it expects that the executable is called `qemu-system-x86_64`.

With `HERMIT_ISLE=uhyve`, the application will be started within a thin
hypervisor powered by Linux's KVM API and therefore requires *KVM* support.
uhyve has a considerably smaller startup time than QEMU, but lacks some features
such as GDB debugging.
In principle, it is an extension of [ukvm](https://www.usenix.org/sites/default/files/conference/protected-files/hotcloud16_slides_williams.pdf).

In this context, the environment variable `HERMIT_CPUS` specifies the number of
cpus (and no longer a range of core ids). Furthermore, the variable `HERMIT_MEM`
defines the memory size of the virtual machine. The suffix of *M* or *G* can be
used to specify a value in megabytes or gigabytes respectively. Per default, the
loader initializes a system with one core and 2 GiB RAM.
For instance, the following command starts the stream benchmark in a virtual machine, which
has 4 cores and 6GB memory.

```bash
$ HERMIT_ISLE=qemu HERMIT_CPUS=4 HERMIT_MEM=6G bin/proxy x86_64-hermit/extra/benchmarks/stream
```

To enable an ethernet device for `uhyve`, we have to setup a tap device on the
host system. For instance, the following command establish the tap device
`tap100` on Linux:

```bash
$ sudo ip tuntap add tap100 mode tap
$ sudo ip addr add 10.0.5.1/24 broadcast 10.0.5.255 dev tap100
$ sudo ip link set dev tap100 up
$ sudo bash -c 'echo 1 > /proc/sys/net/ipv4/conf/tap100/proxy_arp'
```

Per default, `uhyve`'s network interface uses `10.0.5.2`as IP address, `10.0.5.1`
for the gateway and `255.255.255.0` as network mask.
The default configuration could be overloaded by the environment variable
`HERMIT_IP`, `HERMIT_GATEWAY` and `HERMIT_MASk`.
To enable the device, `HERMIT_NETIF` must be set to the name of the tap device.
For instance, the following command starts an HermitCore application within `uhyve`
and enable the network support:

```bash
$ HERMIT_ISLE=uhyve HERMIT_IP="10.0.5.3" HERMIT_GATEWAY="10.0.5.1" HERMIT_MASk="255.255.255.0" HERMIT_NETIF=tap100 bin/proxy x86_64-hermit/extra/tests/hello
```

If `qemu` is used as hyervisor, the virtual machine emulates an RTL8139 ethernet interface and opens at least one TCP/IP ports.
It is used for the communication between HermitCore application and its proxy.
With the environment variable `HERMIT_PORT`, the default port (18766) can be changed for the communication.


### As multi-kernel on a real machine

*Coming soon...*


## Building your own HermitCore applications

You can take `usr/tests` as a starting point to build your own applications. All
that is required is that you include
`[...]/HermitCore/cmake/HermitCore-Application.cmake` in your application's
`CMakeLists.txt`. It doesn't have to reside inside the HermitCore repository.
Other than that, it should behave like normal CMake.


## Debugging

If the application is started via `make qemu`, debugging via GDB is enabled by
default on port 1234. When run via proxy (`HERMIT_ISLE=qemu`), set
`HERMIT_DEBUG=1`.

```
$ gdb x86_64-hermit/extra/tests/hello
(gdb) target extended-remote :1234
Remote debugging using :1234
0xffffffff8100b542 in ?? ()
```

## Tips

### Optimization

You can configure the `-mtune=name` compiler flag by adding `-DMTUNE=name` to
the `cmake` command when configuring the project.

Please note, if the applications is started within a VM, the hypervisor has to
support the specified architecture name.

If QEMU is started by our proxy and the environment variable `HERMIT_KVM` is set
to `0`, the virtual machine will be not accelerated by KVM. In this case, the
`-mtune` flag should be avoided.

### TCP connections

With the environment variable `HERMIT_APP_PORT`, an additional port can be open
to establish an TCP/IP connection with your application.

### Dumping the kernel log

By setting the environment variable `HERMIT_VERBOSE` to `1`, the proxy prints at
termination the kernel log messages onto the screen.

### Network tracing

By setting the environment variable `HERMIT_CAPTURE_NET` to `1` and
`HERMIT_ISLE` to `qemu`, QEMU captures the network traffic and creates the trace
file *qemu-vlan0.pcap*. For instance with [Wireshark](https://www.wireshark.org)
you are able to analyze the file.

### Monitor

If `HERMIT_MONITOR` is set to `1` and `HERMIT_ISLE` to `qemu`, QEMU establishes
a monitor which is available via telnet at port 18767.
With the environment variable `HERMIT_PORT`, the default port (18766) can be changed for the communication between the HermitCore application and its proxy.
The connection to the system monitor is automatically set to `HERMIT_PORT+1`, i.e., the default port is 18767.

## Credits

HermitCore's Emoji is provided free by [EmojiOne](https://www.gfxmag.com/crab-emoji-vector-icon/).
