# rusty-loader

**rusty-loader** is a loader to run [RustyHermit](https://github.com/hermitcore/libhermit-rs) within [Qemu](https://www.qemu.org).
To build the loader the llvm-tools and the source code of Rust's runtime are required and can be installed with following command:

```bash
$ rustup component add rust-src
$ rustup component add llvm-tools-preview
```

You also need `nasm` and `ar` installed on your machine.

Afterwards, the loader can be build as follows:

```bash
$ make
```

Afterwards, the loader is stored in `target/x86_64-unknown-hermit-loader/debug/` as `rusty-loader`.
As final step the unikernel application `app` can be booted with following command:

```bash
$ qemu-system-x86_64 -display none -smp 1 -m 64M -serial stdio  -kernel path_to_loader/rusty-loader -initrd path_to_app/app -cpu qemu64,apic,fsgsbase,rdtscp,xsave,fxsr
```

It is important to enable the processor features _fsgsbase_ and _rdtscp_ because it is a prerequisite to boot RustyHermit.

Please read the README of [RustyHermit](https://github.com/hermitcore/libhermit-rs) for more information.

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
