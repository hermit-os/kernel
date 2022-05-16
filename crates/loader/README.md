# rusty-loader

**rusty-loader** is a loader to run [RustyHermit](https://github.com/hermitcore/libhermit-rs) within [Qemu](https://www.qemu.org).

## Requirements

* [`rustup`](https://www.rust-lang.org/tools/install)
* [NASM](https://nasm.us/) (only for x86_64)

## Building

```bash
$ cargo xtask build --arch x86_64
```

Afterwards, the loader is located at `target/x86_64/debug/rusty-loader`.

## Running

Boot a hermit application:

```bash
qemu-system-x86_64 -smp 1 \
    -cpu qemu64,apic,fsgsbase,rdtscp,xsave,xsaveopt,fxsr \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -display none -m 64M -serial stdio -enable-kvm \
    -kernel path_to_loader/rusty-loader \
    -initrd path_to_app/app
```

It is important to enable the processor features _fsgsbase_ and _rdtscp_ because it is a prerequisite to boot RustyHermit.

Please read the README of [RustyHermit](https://github.com/hermitcore/libhermit-rs) for more information.

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
