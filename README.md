# HermitCore - A lightweight unikernel for a scalable and predictable runtime behavior

The project [HermitCore](http://www.hermitcore.org) is new [unikernel](http://unikernel.org) targeting a scalable and predictable runtime for high-performance computing and cloud computing.
HermitCore extends the multi-kernel approach (like [McKernel](http://www-sys-aics.riken.jp/ResearchTopics/os/mckernel.html)) with unikernel features for a better programmability and scalability for hierarchical systems.
On the startup of HermitCore applications, cores are isolated from the Linux system enabling the bare-metal of the applications on these cores.
This approach achieves lower OS jitter and a better scalability compared to full-weight kernels.
Inter-kernel communication between HermitCore applications and the Linux system is realized by means of an IP interface.

In addition to the multi-kernel approach described above, HermitCore can be used as classical standalone unikernel as well.
In this case HermitCore run a single-kernel exklusive on the hardware or within a virtual machine.
This reduces the demand on resources and improves the boot time, which is an excellent behavior for cloud computing.
It is the result of a research project at RWTH Aachen University and is currently an experimental approach, i.e., not production ready.
Please use it with caution.

## Requirements

The build process works currently only on **x86-based Linux** systems. The following software packets are required to build HermitCore on a Linux system:

* Netwide Assembler (NASM)
* GNU Make, GNU Binutils
* Tools and libraries to build *linux*, *binutils* and *gcc* (e.g. flex, bison, MPFR library, GMP library, MPC library, ISL library)
* texinfo
* Qemu

On Debian-based systems the packets can be installed by executing:
```
  sudo apt-get install qemu-system-x86  nasm texinfo libmpfr-dev libmpc-dev libgmp-dev libisl-dev flex bison
```

## Installing HermitCore with help of debian packets

We provide binary packets for Debian-based systems, which contains the whole HermitCore toolchain including a cross-compiler.
To install the debian packets with following commands:
```
echo "deb [trusted=yes] https://dl.bintray.com/rwth-os/hermitcore vivid main" | sudo tee -a /etc/apt/sources.list
sudo apt-get -qq update
sudo apt-get install binutils-hermit newlib-hermit  pthread-embedded-hermit gcc-hermit libhermit
```
This toolchain is able to build applications to run within VM as [classical unikernel](building-and-testing-hermitcore-as-classical-standalone-unikernel) or bare-metal in a multi-kernel environment.
For the multi-kernel environment, install the a modified Linux kernel.
An introduction is published in section [Building and testing HermitCore as multi-kernel on a real machine](building-and-testing-hermitcore-as-multi-kernel-on a-real-machine).

## Building and testing HermitCore as multi-kernel within a virtual machine

1. Please make sure that you cloned this repository and all its submodules.
2. To configure the system, run the *configure* script in the directory, which contains this *README*.
   With the flag `--with-toolchain`, the HermitCore's complete cross toolchain (cross compiler, binutils, etc.) will be downloaded and built.
   **NOTE**: This requires write access to the installation directory, which is specified by the flag `--prefix`.
   At the end of this *README* in section *Tips* you find hints to enable optimization for the target.
3. The command `make all` build the the HermitCore kernel and depending on the configuration flags the cross toolchain.
4. Install the kernel with `make install`.
5. Build all example applications with `make examples`.
6. To start a virtual machine and to boot a small Linux version use the command `make qemu`.
   Per default, the virtual machine has 10 cores, 2 NUMA nodes, and 8 GiB RAM.
   To increase or to decrease the machine size, the label `qemu` in the Makefile has to be modified accordingly.
7. Inside the VM runs a small Linux system, which already includes the patches for HermitCore.
   Per NUMA node (= HermitCore isle) there is a directory called `isleX` under `/sys/hermit` , where `X` represents the NUMA node ID.
   The demo applications are located in the directories `/hermit/usr/{tests,benchmarks}`.
   A HermitCore loader is already registered.
   By starting a HermitCore application, a proxy will be executed on the Linux system, while the HermitCore binary will be started on isle 0 with cpu 1.
   To change the default behavior, the environment variable `HERMIT_ISLE` is used to specify the (memory) location of the isle, while the environment variable `HERMIT_CPUS` is used to specify the cores.
   For instance, `HERMIT_ISLE=1 HERMIT_CPUS="3-5" /hermit/usr/tests/hello` starts a HelloWorld demo on the HermitCore isle 1, which uses the cores 3 to 5.
   The output messages are forwarded to the Linux proxy and printed on the Linux system.
8. HermitCore's kernel messages of `isleX` are available via `cat /sys/hermit/isleX/log`.
9. There is a virtual IP device for the communication between the HermitCore isles and the Linux system (see output of `ifconfig`).
   Per default, the Linux system has the IP address `192.168.28.1`.
   The HermitCore isles starts with the IP address `192.168.28.2` for isle 0 and is increased by one for every isle.
10. More HermitCore applications are available at `/hermit/usr/{tests,benchmarks}` which is a shared directory between the host and QEmu.

## Building and testing HermitCore as multi-kernel on a real machine

*Note*: to launch HermitCore applications, root privileges are required.

1. In principle you have to follow the tutorial above.
   After the configuration, building of the cross-compilers and all example application (Step 5 in the [above tutorial](#building-and-testing-hermitcore-within-a-virtual-machine)), a modified Linux kernel has to be installed.
   Please clone the repository with the [modified Linux kernel](https://github.com/RWTH-OS/linux).
   Afterwards switch to the branch `hermit` for a relative new vanilla kernel or to `centos`, which is compatible to the current CentOS 7 kernel.
   Configure the kernel with `make menuconfig` for your system.
   Be sure, that the option `CONFIG_HERMIT_CORE` in `Processor type and features` is enabled.
2. Install the Linux kernel and its initial ramdisk on your system (see descriptions of your Linux distribution).
   We recommend to disable Linux NO_HZ feature by setting the kernel parameter `nohz=off`.
3. After a reboot of the system, register the HermitCore loader at your system with following command: `echo ":hermit:M:7:\\x42::/path2proyxy/proxy:" > /proc/sys/fs/binfmt_misc/register`, in which `path2proxy` defines the path to the loader.
   You find the loader `proxy` after building the HermiCore sources in the subdirectory `tools` of the directory, which contains this *README*.
4. The IP device between HermitCore and Linux currently does not support IPv6.
   Consequently, disable IPv6 by adding following line to `/etc/sysctl.conf`: `net.ipv6.conf.mmnif.disable_ipv6 = 1`.
5. Per default, the IP device uses a static IP address range.
   Linux has to use `162.168.28.1`, where HermitCore isles start with `192.168.28.2` (isle 0).
   The network manager must be configured accordingly and therefore the file `/etc/sysconfig/network-scripts/ifcfg-mmnif` must be created with the following content:

```
DEVICE=mmnif
BOOTPROTO=none
ONBOOT=yes
NETWORK=192.168.28.0
NETMASK=255.255.255.0
IPADDR=192.168.28.1
NM_CONTROLLED=yes
```
Finally, follow the [above tutorial](#building-and-testing-hermitcore-within-a-virtual-machine) from Step 5.
The demo applications are located in their subdirectories `usr/{tests,benchmarks}`.

## Building and testing HermitCore as classical standalone unikernel

HermitCore applications can be directly started as standalone kernel within a virtual machine.
In this case, [iRCCE](http://www.lfbs.rwth-aachen.de/publications/files/iRCCE.pdf) is not supported.
Please build HermitCore and register the loader in the same way as done for the multi-kernel version (see [Building and testing HermitCore on a real machine](#building-and-testing-hermitcore-on-a-real-machine)).
If the environment variable `HERMIT_ISLE` is set to `qemu`, the application will be started within a VM.
Please note that the loader requires QEMU and uses per default *KVM*.
Furthermore, it expects that the executable is called `qemu-system-x86_64`.
You can adapt the name by setting the environment variable `HERMIT_QEMU`.

In this context, the environment variable `HERMIT_CPUS` specifies the number of cpus (and no longer a range of core ids).
Furthermore, the variable `HERMIT_MEM` defines the memory size of the virtual machine.
The suffix of *M* or *G* can be used to specify a value in megabytes or gigabytes respectively.
Per default, the loader initializes a system with one core and 2 GiB RAM.

The virtual machine opens two TCP/IP ports.
One is used for the communication between HermitCore application and its proxy.
The second port is used to create a connection via telnet to QEMU's system monitor.
With the environment variable `HERMIT_PORT`, the default port (18766) can be changed for the communication between the HermitCore application and its proxy.
The connection to the system monitor used automatically `HERMIT_PORT+1`, i.e., the default port is 18767.

The following example starts the stream benchmark in a virtual machine, which has 4 cores and 6GB memory.
```
HERMIT_ISLE=qemu HERMIT_CPUS=4 HERMIT_MEM=6G usr/benchmarks/stream
```

## Building HermitCore applications

After successful building of HermitCore and its demo applications (see above), HermitCore’s cross toolchain (*gcc*, *g++*, *gfortran*, *gccgo*, *objdump*, etc.) is located at the subdiretory `usr/x86` of the directory, which contains this *README*.
To use these tools, add `usr/x86/bin` to your environment variable `PATH`.
As with any other cross toolchain, the tool names begin with the target architecture (*x86_64*) and the name of the operating system (*hermit*).
For instance, `x86_64-hermit-gcc` stands for the GNU C compiler, which is able to build HermitCore applications.

All tools can be used as the well-known GNU tools. Only the Go compiler works different to the typical workflow.
Instead of building Go application like
```
go build main.go
```
you have to use the compiler as follows
```
x86_64-hermit-gccgo -pthread -Wall -o main main.go
```
For network support, you have to link the Go application with the flag `-lnetgo`.

## Tips

1. The configuration flag `--with-mtune=name` specifies the name of the target processor for which GCC should tune the performance of the code.
   You can use any architecture name, which is supported by GCC.
   For instance, `--with-mtune=native` optimzes the code for the host system.
   Please note, if the applications is started within a VM, the hypervisor has to support the specified architecture name.
   Per default the system will be accelerated by KVM and the host architecture will be used as target processor.
2. If Qemu is started by our proxy and the environment variable `HERMIT_KVM` is set to `0`, the virtual machine will be not accelerated by KVM.
   In this case, the configuration flag `--with-mtune=name` should be avoided.
   With the environment variable `HERMIT_APP_PORT`, an additional port can be open to establish an TCP/IP connection with your application.
3. By setting the environment variable `HERMIT_VERBOSE` to `1`, the proxy prints at termination the kernel log messages onto the screen.
4. If `HERMIT_DEBUG` is set to `1`, Qemu will establish an gdbserver, which will be listen port 1234.
   Afterwards you are able debug HermitCore applications remotely.
