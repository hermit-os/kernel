# HermitCore - A lightweight unikernel for a scalable and predictable runtime behavior

The project [HermitCore](http://www.hermitcore.org) is new [unikernel](http://unikernel.org) targeting at high-performance computing.
HermitCore extends the multi-kernel approach (like [McKernel](http://www-sys-aics.riken.jp/ResearchTopics/os/mckernel.html)) with unikernel features for a better programmability and scalability for hierarchical systems.
On the startup of HermitCore applications, cores are isolated from the Linux system enabling the bare-metal of the applications on these cores.
This approach achieves lower OS jitter and a better scalability compared to full-weight kernels.
Inter-kernel communication between HermitCore applications and the Linux system is realized by means of an IP interface.

In addition to the multi-kernel approach described above, HermitCore can be used as classical standalone unikernel as well.
This reduces the demand on resources and improves the boot time.
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
  sudo apt-get install qemu-system-x86 nasm texinfo libmpfr-dev libmpc-dev libgmp-dev libisl-dev flex bison
```

## Building and testing HermitCore within a virtual machine

0. Please make sure that you cloned this repository and all its submodules.
1. To configure the system, run the *configure* script in the directory, which contains this *README*.
   Fine tuning of the installation directories, e.g., with the flag `--prefix` is currently not supported.
   HermitCore, the cross-compiler and the demo applications will be installed in subdirectories of this repository.
   At the end of this *README* in section *Tips* you find hints to enable optimization for the target.
2. The command `make` build the Linux kernel, the HermitCore kernel, the cross-compiler, and the demo applications.
3. To start a virtual machine and to boot a small Linux version use the command `make qemu`.
   Per default, the virtual machine has 10 cores, 2 NUMA nodes, and 8 GiB RAM.
   To increase or to decrease the machine size, the label `qemu` in the Makefile has to be modified accordingly.
4. Inside the VM runs a small Linux system, which already includes the patches for HermitCore.
   Per NUMA node (= HermitCore isle) there is a directory called `isleX` under `/sys/hermit` , where `X` represents the NUMA node ID.
   The demo applications are located in the directories `/hermit/usr/{tests,benchmarks}`.
   A HermitCore loader is already registered.
   By starting a HermitCore application, a proxy will be executed on the Linux system, while the HermitCore binary will be started on isle 0 with cpu 1.
   To change the default behavior, the environment variable `HERMIT_ISLE` is used to specify the (memory) location of the isle, while the environment variable `HERMIT_CPUS` is used to specify the cores.
   For instance, `HERMIT_ISLE=1 HERMIT_CPUS="3-5" /hermit/usr/tests/hello` starts a HelloWorld demo on the HermitCore isle 1, which uses the cores 3 to 5.
   The output messages are forwarded to the Linux proxy and printed on the Linux system.
5. HermitCore's kernel messages of `isleX` are available via `cat /sys/hermit/isleX/log`.
6. There is a virtual IP device for the communication between the HermitCore isles and the Linux system (see output of `ifconfig`).
   Per default, the Linux system has the IP address `192.168.28.1`.
   The HermitCore isles starts with the IP address `192.168.28.2` for isle 0 and is increased by one for every isle.
7. More HermitCore applications are available at `/hermit/usr/{tests,benchmarks}` which is a shared directory between the host and QEmu.

## Building and testing HermitCore on a real machine

*Note*: to launch HermitCore applications, root privileges are required.

1. In principle you have to follow the tutorial above.
   After the configuration (Step 2 in the [above tutorial](#building-and-testing-hermitcore-within-a-virtual-machine)) go to the subdirectory `linux`, which contains the source code of the Linux kernel.
   Configure the kernel with `make menuconfig` for your system.
   Be sure, that the option `CONFIG_HERMIT_CORE` in `Processor type and features` is enabled.
2. Go back to the root directory of this repository and build with `make` the Linux kernel, the HermitCore kernel, the cross-compiler, and the demo applications.
3. Install the Linux kernel and its initial ramdisk on your system (see descriptions of your Linux distribution).
   We recommend to disable Linux NO_HZ feature by setting the kernel parameter `no_hz=off`.
4. Register the HermitCore loader at your system with following command: `echo ":hermit:M:7:\\x42::/path2proyxy/proxy:" > /proc/sys/fs/binfmt_misc/register`, in which `path2proxy` defines the path to the loader.
   You find the loader `proxy` after building the HermiCore sources in the subdirectory `hermit/tools` of the directory, which contains this *README*.
5. The IP device between HermitCore and Linux currently does not support IPv6.
   Consequently, disable IPv6 by adding following line to `/etc/sysctl.conf`: `net.ipv6.conf.mmnif.disable_ipv6 = 1`.
6. Per default, the IP device uses a static IP address range.
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
Finally, boot your system with the new Linux kernel and follow the [above tutorial](#building-and-testing-hermitcore-within-a-virtual-machine) from Step 5.

The demo applications are located in their subdirectories `hermit/usr/{tests,benchmarks}`.

## HermitCore as classical standalone unikernel

HermitCore applications can be directly started as standalone kernel within a virtual machine.
In this case, [iRCCE](http://www.lfbs.rwth-aachen.de/publications/files/iRCCE.pdf) is not supported.
Please register the loader the same way as done for the multi-kernel version of HermitCore (see Step 4 in [*Building and testing HermitCore on a real machine*](#building-and-testing-hermitcore-on-a-real-machine)).
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
HERMIT_ISLE=qemu HERMIT_CPUS=4 HERMIT_MEM=6G hermit/usr/benchmarks/stream
```

## Tips

1. The configuration flag `--with-mtune=name` specifies the name of the target processor for which GCC should tune the performance of the code.
   You can use any architecture name, which is supported by GCC.
   For instance, `--with-mtune=native` optimzes the code for the host system.
   Please note, if the applications is started within a VM, the hypervisor has to support the specified architecture name.
   If KVM is started by our proxy, per default the host architecture will be used as target processor.
2. If KVM is started by our proxy and the environment variable `HERMIT_KVM` is set to `0`, the virtual machine will be not accelerated by KVM.
   In this case, the configuration flag `--with-mtune=name` should be avoided.
3. By setting the environment variable `HERMIT_VERBOSE` to `1`, the proxy prints at termination the kernel log messages onto the screen.
