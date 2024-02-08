# The Hermit Loader

This project is a loader to run the [Hermit kernel](https://github.com/hermitcore/kernel) within [QEMU](https://www.qemu.org).

## Requirements

* [`rustup`](https://www.rust-lang.org/tools/install)

## Building

```bash
$ cargo xtask build --target <TARGET> --release
```

With `<TARGET>` being either `x86_64`, `x86_64-uefi`, or `aarch64`.

Afterward, the loader is located at `target/<TARGET>/release/hermit-loader`.

## Running

### x86-64

On x86-64 Linux with KVM, you can boot Hermit like this:

```
$ qemu-system-x86_64 \
    -enable-kvm \
    -cpu host \
    -smp 1 \
    -m 128M \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -display none -serial stdio \
    -kernel <LOADER> \
    -initrd <APP>
```

#### No KVM

If you want to emulate x86-64 instead of using KVM, omit `-enable-kvm` and set the CPU explicitly to a model of your choice, for example `-cpu Skylake-Client`.

#### Benchmarking

If you want to benchmark Hermit, make sure to enable the _invariant TSC_ (`invtsc`) feature by setting `-cpu host,migratable=no,+invtsc,enforce`.

#### Providing Arguments

Unikernel arguments can be provided like this:

```
$ qemu-system-x86_64 ... \
    -append "[KERNEL_ARGS] [--] [APP_ARGS]"
```

### AArch64

On AArch64, the base command is as follows:

```
$ qemu-system-aarch64 \
                  -machine virt,gic-version=3 \
                  -cpu cortex-a76 \
                  -smp 1 \
                  -m 512M  \
                  -semihosting \
                  -display none -serial stdio \
                  -kernel <LOADER> \
                  -device guest-loader,addr=0x48000000,initrd=<APP>
```

### Debugging

You can use QEMU to debug the loaded Hermit images:

1.  Start your Hermit image normally.

    Look for the following line:

    ```log
    [LOADER][INFO] Loading kernel to <START>..<END> (len = <LEN> B)
    ```

    We need to know `<START>` to tell GDB later where the program is loaded.

2.  Add `-S -s` to your QEMU command.

    `-S` makes QEMU start with a stopped CPU, which can be started explicitly.
    `-s` is a shorthand for `-gdb tcp::1234` for accepting GDB connections.

3.  Start GDB without arguments.

    You should use the `rust-gdb` or `rust-gdbgui` wrappers for Rust's pretty printing.
    Both respect the `RUST_GDB` environment variable for cross-debugging (e.g., `aarch64-elf-gdb`).

4.  Connect to QEMU.

    ```gdb
    target remote :1234
    ```

5.  Load the Hermit image to the correct address.

    We can now tell GDB where the Hermit image will be located:

    ```gdb
    symbol-file -o <START> <IMAGE_PATH>
    ```

6.  Debug away!

    You can now add breakpoints and start execution:
    
    ```gdb
    b hermit::boot_processor_main
    c
    ```

    For fast iteration times, consider creating a [`.gdbinit`](https://sourceware.org/gdb/onlinedocs/gdb/gdbinit-man.html).


### Using QEMU as microvm

QEMU provides the [microvm virtual platform], which is a minimalist machine type without PCI nor ACPI support.
Microvms have a smaller memory footprint and a faster boot time.

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

To enable an Ethernet device, we have to set up a tap device on the host system.
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

Currently, Hermit only supports [Virtio]:

[Virtio]: https://www.redhat.com/en/blog/introduction-virtio-networking-and-vhost-net

```
$ qemu-system-x86_64 ... \
    -netdev tap,id=net0,ifname=tap10,script=no,downscript=no,vhost=on \
    -device virtio-net-pci,netdev=net0,disable-legacy=on
```

You can now access the files in `SHARED_DIRECTORY` under the virtiofs tag like `/myfs/testfile`.

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
