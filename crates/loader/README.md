# RustyLoader

**RustyLoader** is a loader to run [RustyHermit](https://github.com/hermitcore/libhermit-rs) within [QEMU](https://www.qemu.org).

## Requirements

* [`rustup`](https://www.rust-lang.org/tools/install)
* [NASM](https://nasm.us/) (only for x86_64)

## Building

```bash
$ cargo xtask build --target x86_64
```

Afterwards, the loader is located at `target/x86_64/debug/rusty-loader`.

## Running

Boot a hermit application:

```
$ qemu-system-x86_64 \
    -cpu qemu64,apic,fsgsbase,fxsr,rdrand,rdtscp,xsave,xsaveopt \
    -smp 1 -m 64M \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -display none -serial stdio \
    -kernel <LOADER> \
    -initrd <APP>
```

Arguments can be provided like this:

```
$ qemu-system-x86_64 ... \
    -append "[KERNEL_ARGS] [--] [APP_ARGS]"
```

### Using QEMU as microvm

QEMU provides the [microvm virtual platform], which is a minimalist machine type without PCI nor ACPI support.
Microvms have smaller memory footprint and a faster boot time.

[microvm virtual platform]: https://qemu.readthedocs.io/en/latest/system/i386/microvm.html

To use this VM type, PCI and ACPI support have to be disabled for your app (using `no-default-features`).

```
$ qemu-system-x86_64 ... \
    -M microvm,x-option-roms=off,pit=off,pic=off,rtc=on,auto-kernel-cmdline=off \
    -nodefaults -no-user-config \
    -append "-freq 2800"
```

Depending on the virtualized processor, the processor frequency has to be passed as kernel argument (`-freq`, in MHz).

### Network support

To enable an ethernet device, we have to set up a tap device on the host system.
The following commands establish the tap device `tap10` on Linux:

```
# ip tuntap add tap10 mode tap
# ip addr add 10.0.5.1/24 broadcast 10.0.5.255 dev tap10
# ip link set dev tap10 up
# echo 1 > /proc/sys/net/ipv4/conf/tap10/proxy_arp
```

If you want Hermit to be accessible from outside the host, you have to enable IP forwarding:
```
# sysctl -w net.ipv4.ip_forward=1
```

You need to enable the `tcp` feature of the kernel.

The network configuration can be set via environment variables during compile time.
By default, it is:

```
HERMIT_IP="10.0.5.3"
HERMIT_GATEWAY="10.0.5.1"
HERMIT_MASK="255.255.255.0"
```

Currently, Hermit only supports [virtio]:

[virtio]: https://www.redhat.com/en/blog/introduction-virtio-networking-and-vhost-net

```
$ qemu-system-x86_64 ... \
    -netdev tap,id=net0,ifname=tap10,script=no,downscript=no,vhost=on \
    -device virtio-net-pci,netdev=net0,disable-legacy=on
```

You can now access the files in SHARED_DIRECTORY under the virtiofs tag like `/myfs/testfile`.

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
