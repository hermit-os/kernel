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

- **Rust applications:** Instructions can be found in the [hermit-rs](https://github.com/hermit-os/hermit-rs) repository.
- **For C/C++ applications:** Instructions can be found in the [hermit-c](https://github.com/hermit-os/hermit-c) repository.
 

### Standalone static library build

```sh
cargo xtask build --arch x86_64
```

On completion, the script will print the path of `libhermit.a`.
If you want to build the kernel for aarch64, please replace `x86_64` by `aarch64`.
If you want to build the kernel for riscv64, please use `riscv64`. 

### Control the kernel messages verbosity

This kernel uses the lightweight logging crate [log](https://github.com/rust-lang/log) to print kernel messages. The
compile time environment variable `HERMIT_LOG_DEFAULT` and the runtime environment variable `HERMIT_LOG` control the
verbosity and follow [the env_logger format](https://docs.rs/env_logger/latest/env_logger/) but without the regex
support.

The logging level can be changed per module by setting it to a string in the format `[target][=level][,...]`, where the
level is a string matching the name of a [LevelFilter](https://docs.rs/log/0.4.8/log/enum.LevelFilter.html). If the
target is omitted, the level is set as the global level. If the level is omitted, logs of all levels are printed for the
target. A simple search pattern that will filter all modules can be provided after the target-level pairs with 
`/<pattern>`.

> [!NOTE]
> For the modules that are part of the kernel, the `hermit::` prefix needs to be provided before the module name.

If the variables are not set, or they do not provide a global level, then `LevelFilter::Info` is used as the global level
by default.

```sh
HERMIT_LOG_LEVEL_FILTER='hermit::virtio=debug/queue' cargo xtask build --arch x86_64
```

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
