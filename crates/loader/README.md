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

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
