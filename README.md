<img width="100" align="right" src="img/hermitcore_logo.png" />


# HermitCore - A lightweight unikernel for a scalable and predictable runtime behavior

[![Build Status](https://travis-ci.org/RWTH-OS/HermitCore.svg?branch=devel)](https://travis-ci.org/RWTH-OS/HermitCore)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

The project [HermitCore]( http://www.hermitcore.org ) is a new
[unikernel](http://unikernel.org) targeting a scalable and predictable runtime
for high-performance and cloud computing. HermitCore extends the multi-kernel
approach (like
[McKernel](http://www-sys-aics.riken.jp/ResearchTopics/os/mckernel.html)) with
unikernel features for a better programmability and scalability for hierarchical
systems.

![HermitCore Demo](img/demo.gif)

On the startup of HermitCore applications, cores are isolated from the Linux
system enabling bare-metal execution of on these cores. This approach achieves
lower OS jitter and a better scalability compared to full-weight kernels.
Inter-kernel communication between HermitCore applications and the Linux system
is realized by means of an IP interface.

In addition to the multi-kernel approach described above, HermitCore can be used
as a classical standalone unikernel as well. In this case, HermitCore runs a
single-kernel exclusively on the hardware or within a virtual machine. This
reduces the resource demand and lowers the boot time which is critical for
cloud computing applications. It is the result of a research project at RWTH
Aachen University and is currently an experimental approach, i.e., not
production ready. Please use it with caution.

## Contributing

HermitCore is being developed on [GitHub](https://github.com/RWTH-OS/HermitCore).
Create your own fork, send us a pull request, and chat with us on [Slack](https://radiant-ridge-95061.herokuapp.com).

## Requirements

The build process works currently only on **x86-based Linux** systems. To build
the HermitCore kernel and applications you need:

 * CMake
 * Netwide Assember (NASM)
 * recent host compiler such as GCC
 * HermitCore cross-toolchain, i.e. Binutils, GCC, newlib, pthreads

### HermitCore cross-toolchain

We provide prebuilt packages (currently Debian-based only) of the HermitCore
toolchain, which can be installed as follows:

```bash
$ echo "deb [trusted=yes] https://dl.bintray.com/rwth-os/hermitcore vivid main" | sudo tee -a /etc/apt/sources.list
$ sudo apt-get -qq update
$ sudo apt-get install binutils-hermit newlib-hermit pthread-embedded-hermit gcc-hermit libhermit
```

For non-Debian based systems, a docker image with the complete toolchain is provided and can be installed as follows:

```bash
$ docker pull rwthos/hermitcore
```

The following commad starts within the new docker container a shell and mounts from the host system the directory `~/src` to `/src`:

```bash
$ docker run -i -t -v ~/src:/src rwthos/hermitcore:latest
```

Within the shell the cross-toolchain can be used to build HermitCore applications.

If you want to build the toolchain yourself, have a look at the repository [hermit-toolchain](https://github.com/RWTH-OS/hermit-toolchain), which contains scripts to build the whole toolchain.

Depending on how you want to use HermitCore, you might need additional packages
such as:

 * QEMU (`apt-get install qemu-system-x86`)

## Building HermitCore

### Preliminary work

To build HermitCore from source (without compiler), the repository with its submodules has to be cloned.

```bash
$ git clone git@github.com:RWTH-OS/HermitCore.git
$ cd HermitCore
$ git submodule init
$ git submodule update
```

We require a fairly recent version of CMake (`3.7`) which is not yet present in
most Linux distributions. We therefore provide a helper script that fetches the
required CMake binaries from the upstream project and stores them locally, so
you only need to download it once.

```bash
$ . cmake/local-cmake.sh
-- Downloading CMake
--2017-03-28 16:13:37--  https://cmake.org/files/v3.7/cmake-3.7.2-Linux-x86_64.tar.gz
Loaded CA certificate '/etc/ssl/certs/ca-certificates.crt'
Resolving cmake.org... 66.194.253.19
Connecting to cmake.org|66.194.253.19|:443... connected.
HTTP request sent, awaiting response... 200 OK
Length: 30681434 (29M) [application/x-gzip]
Saving to: ‘cmake-3.7.2-Linux-x86_64.tar.gz’

cmake-3.7.2-Linux-x86_64.tar.gz         100%[===================>]  29,26M  3,74MB/s    in 12s     

2017-03-28 16:13:50 (2,48 MB/s) - ‘cmake-3.7.2-Linux-x86_64.tar.gz’ saved [30681434/30681434]

-- Unpacking CMake
-- Local CMake v3.7.2 installed to cmake/cmake-3.7.2-Linux-x86_64
-- Next time you source this script, no download will be necessary
```

So before you build HermitCore you have to source the `local-cmake.sh` script
everytime you open a new terminal.

### Building the library operating systems and its examples

To build HermitCore go to the directory with the source code, create a `build` directory, and call in the new directory `cmake` followed by `make`.

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
virtual machine. In this case,
[iRCCE](http://www.lfbs.rwth-aachen.de/publications/files/iRCCE.pdf ) is not
supported.

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


### As multi-kernel within a virtual machine

Boot the test image of a minimal Linux system within a VM.
For this, go to the build directory and boot the image by our makefiles.

```bash
$ cd build
$ make qemu
$ # or 'make qemu-dep' to build HermitCore dependencies before
```

Within the QEMU session you can start HermitCore application just the same as
traditional Linux programs:

```bash
(QEMU) $ /hermit/x86_64-hermit/extra/tests/hello
smpboot: CPU 1 is now offline
Hello World!!!
argv[0] = /hermit/x86_64-hermit/extra/tests/hello
Receive signal with number 30
Hostname: hermit.localdomain
x86: Booting SMP configuration:
smpboot: Booting Node 0 Processor 1 APIC 0x1
```

Per default, the virtual machine has 10 cores, 2 NUMA nodes, and 8 GiB RAM.
Inside the VM runs a small Linux system, which already includes the patches for
HermitCore. Per NUMA node (= HermitCore isle) there is a directory called
`isleX` under `/sys/hermit` , where `X` represents the NUMA node ID.

The demo applications are located in the directories
`/hermit/x86_64-hermit/extra/{tests,benchmarks}`. A HermitCore loader is already registered.
By starting a HermitCore application, a proxy will be executed on the Linux
system, while the HermitCore binary will be started on isle 0 with cpu 1. To
change the default behavior, the environment variable `HERMIT_ISLE` is used to
specify the (memory) location of the isle, while the environment variable
`HERMIT_CPUS` is used to specify the cores.

For instance, `HERMIT_ISLE=1 HERMIT_CPUS="3-5" /hermit/x86_64-hermit/extra/tests/hello` starts
a HelloWorld demo on the HermitCore isle 1, which uses the cores 3 to 5. The
output messages are forwarded to the Linux proxy and printed on the Linux
system.

HermitCore's kernel messages of `isleX` are available via `cat
/sys/hermit/isleX/log`.

There is a virtual IP device for the communication between the HermitCore isles
and the Linux system (see output of `ifconfig`). Per default, the Linux system
has the IP address `192.168.28.1`. The HermitCore isles starts with the IP
address `192.168.28.2` for isle 0 and is increased by one for every isle.

More HermitCore applications are available at `/hermit/usr/{tests,benchmarks}`
which is a shared directory between the host and QEMU.


### As multi-kernel on a real machine

*Note*: to launch HermitCore applications, root privileges are required.

A [modified Linux kernel](https://github.com/RWTH-OS/linux) has to be installed.
Afterwards switch to the branch `hermit` for a relative new vanilla kernel or to
`centos`, which is compatible to the current CentOS 7 kernel. Configure the
kernel with `make menuconfig` for your system. Be sure, that the option
`CONFIG_HERMIT_CORE` in `Processor type and features` is enabled.

```bash
$ git clone https://github.com/RWTH-OS/linux
$ cd linux
$ # see comments above
$ git checkout hermit
$ make menuconfig
$ make
```

Install the Linux kernel and its initial ramdisk on your system (see
descriptions of your Linux distribution). We recommend to disable Linux NO_HZ
feature by setting the kernel parameter `nohz=off`.

Install HermitCore to your system (by default to `/opt/hermit`):

```bash
$ cd build
$ sudo make install
$ ls -l /opt/hermit
```

After a reboot of the system, register the HermitCore loader at your system with
following command:

```bash
$ sudo -c sh 'echo ":hermit:M:7:\\x42::/opt/hermit/bin/proxy:" > /proc/sys/fs/binfmt_misc/register'
```

The IP device between HermitCore and Linux currently does not support IPv6.
Consequently, disable it (might be slightly different on your distribution):

```bash
$ echo 'net.ipv6.conf.mmnif.disable_ipv6 = 1' | sudo tee /etc/sysctl.conf
```

Per default, the IP device uses a static IP address range. Linux has to use
`162.168.28.1`, where HermitCore isles start with `192.168.28.2` (isle 0). The
interface is `mmnif`.

Please configure your network accordingly. For CentOS, you have to create the
file `/etc/sysconfig/network-scripts/ifcfg-mmnif`:

```
DEVICE=mmnif
BOOTPROTO=none
ONBOOT=yes
NETWORK=192.168.28.0
NETMASK=255.255.255.0
IPADDR=192.168.28.1
NM_CONTROLLED=yes
```

You can now start applications the same way as from within a virtual machine
(see description above).


## Building your own HermitCore applications

You can take `usr/tests` as a starting point to build your own applications. All
that is required is that you include
`[...]/HermitCore/cmake/HermitCore-Application.cmake` in your application's
`CMakeLists.txt`. It doesn't have to reside inside the HermitCore repository.
Other than that, it should behave like normal CMake.


## Profiling

We provide profiling support via the XRay profiler. See `usr/xray/README.md` for
more information on how to use it.


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
