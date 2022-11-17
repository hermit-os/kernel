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

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
