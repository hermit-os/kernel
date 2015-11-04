# HermitCore - A lightweight extreme-scale satellite kernel

The project has just initiated. Further information will be published shortly.

## Requirements

* Netwide Assembler (NASM)
* GNU Make, GNU Binutils
* Tools and libraries to build *linux*, *binutils* and *gcc* (e.g. flex, bison, MPFR library, GMP library, MPC library, ISL library)
* genisoimage
* texinfo
* Qemu

## Building and testing HermitCore within a virtual machine

1. The build process does only work on a x86-based Linux system.
2. To configure the system, run the *configure* script in the dirctory, which contains this *README*. Fine tuning of the installation directories e.g. with the flag `--prefix` is currently not supported. HermitCore, the cross-compiler and the demo applications will be installed in subdirectories of this repository.
3. Build the Linux kernel, the HermitCore kernel, the corss-compiler and the demo applications with `make`.
4. Create a virtual machine and boot a small Linux version with `make qemu`. Per default, the virtual machine has 20 cores, 4 NUMA nodes and 8 GByte RAM. To increase or to decrease the machine size, the label `qemu` in the Makefile has to be modified.
5. Afterwards, a small Linux system should run, which already includes the patches for HermitCore. For each NUMA node (= HermitCore isle) is in `/sys/hermit` a directory `isleX`, where `X` represents the number of the NUMA node. In this environment, Linux uses only one core. Consequently, 20-1 cores could be mapped to HermitCore isles. For instance, `echo 1-4 > /sys/hermit/isle0/cpus` boots a HermitCore kernel on core 1-4. The required memory will be allocated from NUMA node 0 (= `isle0`).
6. HermitCore's kernel messages of `isle0` are available via `cat /sys/hermit/isle0/log`.
7. It exists an virtual IP devices between HermitCore isles and the Linux system (see output of `ifconfig`). Per default, the Linux system has the IP address `192.168.28.1`. The HermitCore isles starts with the IP address `192.168.28.2` for isle 0 and is increased by one for every isle. Please test the connection to isle 0 with `ping -c 5 192.168.28.2`.
8. All demo applications are mapped into the directory `/hermit` of the Linux system. All applications are Linux applications with integrated HermitCore binaries. By starting of an application, a Linux process will be created, which sends the HermitCore binary via TCP/IP per default to isle 0. With the environment variable HERMIT_ISLE, the default behavior could be overloaded. The HermitCore isle receives the binary, starts the applications and forward all output messages to the Linux system. For instance, `HERMIT_ISLE=0 /hermit/hello_proxy` starts the classical *HelloWorld* demo on isle 0 and the output messages are printed on the Linux system.

## Building and testing HermitCore on a real machine

1. In principle you have to follow the tutorial above. After the configuration (step 2 in the above tutorial) go to the subdirectory `linux`, which contains the source code of the Linux kernel. Configure the kernel with `make menuconfig` for your system. Be sure, that the option `CONFIG_HERMIT_CORE` in `Processor type and features` is enabled.
2. Go back to the root directory of this repository and build with `make` the Linux kernel, the HermitCore kernel, the cross-compiler and the demo applications.
3. Install the Linux kernel and its initial ramdisk on your system (see descriptions of your Linux distribution).
4. Create the directory `hermit` in the root directory of your Linux system (`mkdir /hermit`).
5. Copy the HermitCore kernel and the demo applications to the new directory (`cp hermit/hermit.bin /hermit ; cp hermit/tools/iso/* /hermit`).
6. The IP device between HermitCore and Linux does currently not support IPv6. Consequently, disable IPv6 by adding following line to `/etc/sysctl.conf`: `net.ipv6.conf.eth0.disable_ipv6 = 1`.
7. Per default, the IP device uses a static IP address range. Linux has to use `162.168.28.1`, where HermitCore isles start with `192.168.28.2` (isle 0). The network manager must be configured accordingly and consequently the file `/etc/sysconfig/network-scripts/ifcfg-mmnif` must be created with following contents:
```
DEVICE=mmnif
BOOTPROTO=none
ONBOOT=yes
NETWORK=192.168.28.0
NETMASK=255.255.255.0
IPADDR=192.168.28.1
NM_CONTROLLED=yes
```
Finally, boot your system with the new Linux kernel and follow the above tutorial (*Building and testing HermitCore within a virtual machine*) from point 5.
