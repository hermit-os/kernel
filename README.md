<img width="256" align="right" src="https://github.com/hermit-os/.github/blob/main/logo/hermit-logo.svg" />

# Hermit Kernel

[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg)](https://hermit-os.github.io/kernel)
![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)
[![Zulip Badge](https://img.shields.io/badge/chat-hermit-57A37C?logo=zulip)](https://hermit.zulipchat.com/)
[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.14645534.svg)](https://doi.org/10.5281/zenodo.14645534)

This is the kernel of the [Hermit](https://github.com/hermit-os) unikernel project.

For details, see the [docs].

[docs]: https://hermit-os.github.io/kernel

## Requirements

* [`rustup`](https://www.rust-lang.org/tools/install)

## Building the kernel

Usually the kernel will be linked as static library to your applications.

- **Rust applications:** Examples can be found in [`examples`](examples), see [Running an example](#running-an-example).
  For your own applications, have a look at [the template](https://github.com/hermit-os/hermit-rs-template).
- **For C/C++ applications:** Instructions can be found in the [hermit-c](https://github.com/hermit-os/hermit-c) repository.
 

### Standalone static library build

```sh
cargo xtask build --arch x86_64
```

On completion, the script will print the path of `libhermit.a`.
If you want to build the kernel for aarch64, please replace `x86_64` by `aarch64`.
If you want to build the kernel for riscv64, please use `riscv64`. 

### Control the kernel messages verbosity

This kernel uses the lightweight logging crate [log](https://github.com/rust-lang/log) to print kernel messages.
The environment variable `HERMIT_LOG_LEVEL_FILTER` controls the verbosity and follows [the env_logger format](https://docs.rs/env_logger/latest/env_logger/) but without the regex support.
You can change it per module (including its submodules) by setting it to a string in the format `[target][=level][,...]`, where the level is a string matching the name of a [LevelFilter](https://docs.rs/log/0.4.8/log/enum.LevelFilter.html).
If the target is omitted, the level is set as the global level. If the level is
omitted, logs of all levels are printed for the target. A substring search
pattern that will filter all modules can be provided after the target-level
pairs with `/<pattern>`.
If the variable is not set, cannot be parsed, or it does not provide a global level, then a global level `LevelFilter::Info` is used by default.

> [!NOTE]
> Although the logger only prints name of the lowest level for the kernel
  modules, the match filter requires paths to start from the root, i.e. in the
  format `hermit::MODULE_NAME::SUBMODULE_NAME`.

```sh
$ HERMIT_LOG_LEVEL_FILTER="hermit::drivers::net=debug,smoltcp,error" cargo xtask build --arch x86_64
```

## Running an example

The directory [`examples`](examples) contains Rust applications, which are linked with the kernel by the crate [`hermit`](crates/hermit).
Running them requires [QEMU](https://www.qemu.org/) and the [Hermit loader](crates/loader).
The following steps build and run [`hello_world`](examples/hello_world) on x86_64.

1. Build the loader:

   ```sh
   cd crates/loader
   cargo xtask build --target x86_64-multiboot --release
   cd ../..
   ```

   The loader is located at `crates/loader/target/release/hermit-loader-x86_64-multiboot`.

2. Build the application together with the kernel:

   ```sh
   cargo build -Zbuild-std=std,panic_abort --target x86_64-unknown-hermit --release --package hello_world --features hermit/loader
   ```

   The application is located at `target/x86_64-unknown-hermit/release/hello_world`.

3. Run the application in QEMU:

   ```sh
   qemu-system-x86_64 -cpu Skylake-Client -smp 1 -m 512M \
       -display none -serial stdio \
       -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
       -kernel crates/loader/target/release/hermit-loader-x86_64-multiboot \
       -initrd target/x86_64-unknown-hermit/release/hello_world
   ```

   The application prints `Hello, world!`, and the kernel reports `exit status 0`.
   Because of the `isa-debug-exit` device, QEMU itself exits with code 3 on success.
   With KVM, replace `-cpu Skylake-Client` by `-accel kvm -cpu host`.

On aarch64, build the loader with `--target aarch64-elf` and the application with `--target aarch64-unknown-hermit`.
The application is passed to the loader as a separate image:

```sh
qemu-system-aarch64 -machine virt,gic-version=3 -cpu max,lpa2=off -smp 1 -m 512M \
    -display none -serial stdio -semihosting \
    -kernel crates/loader/target/release/hermit-loader-aarch64-elf \
    -device guest-loader,addr=0x48000000,initrd=target/aarch64-unknown-hermit/release/hello_world
```

### Running examples with xtask

The CI builds and runs the examples with `cargo xtask ci rs`, which also configures QEMU for the requested devices.
It expects the loader in the root directory of the repository:

```sh
cp crates/loader/target/release/hermit-loader-x86_64-multiboot .
cargo xtask ci rs --arch x86_64 --features hermit/loader --package hello_world qemu
```

Further examples, such as networking or file systems, need additional features and devices, e.g.:

```sh
cargo xtask ci rs --arch x86_64 --features hermit/loader --package httpd --features ci,hermit/dhcpv4,hermit/virtio-net qemu --devices virtio-net-pci
```

See [`.github/workflows/ci.yml`](.github/workflows/ci.yml) for the combinations tested by the CI.

## Credits

This kernel is derived from following tutorials and software distributions:

1. Philipp Oppermann's [excellent series of blog posts][opp].
2. Erik Kidd's [toyos-rs][kidd], which is an extension of Philipp Opermann's kernel.
3. The Rust-based teaching operating system [eduOS-rs][eduos].

[opp]: http://blog.phil-opp.com/
[kidd]: http://www.randomhacks.net/bare-metal-rust/
[eduos]: http://rwth-os.github.io/eduOS-rs/

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

The kernel is being developed on [hermit-os/kernel](https://github.com/hermit-os/kernel).
Create your own fork, send us a pull request, and chat with us on [Zulip](https://hermit.zulipchat.com/).
