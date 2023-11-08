<img width="256" align="right" src="https://github.com/hermit-os/.github/blob/main/logo/hermit-logo.svg" />

# Hermit Kernel

[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg)](https://hermit-os.github.io/kernel/hermit/)
![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)
[![Zulip Badge](https://img.shields.io/badge/chat-hermit-57A37C?logo=zulip)](https://hermit.zulipchat.com/)

This is the kernel of the [Hermit](https://github.com/hermit-os) unikernel project.

## Requirements

* [`rustup`](https://www.rust-lang.org/tools/install)

## Building the kernel

Usually the kernel will be linked as static library to your applications.

- **Rust applications:** Instructions can be found in the [hermit-rs](https://github.com/hermit-os/hermit-rs) repository.
- **For C/C++ applications:** Instructions can be found in the [hermit-playground](https://github.com/hermit-os/hermit-playground) repository.
 

### Standalone static library build

```sh
cargo xtask build --arch x86_64
```

On completion, the script will print the path of `libhermit.a`.
If you want to build the kernel for aarch64, please replace `x86_64` by `aarch64`.

### Control the kernel messages verbosity

This kernel uses the lightweight logging crate [log](https://github.com/rust-lang/log) to print kernel messages.
The environment variable `HERMIT_LOG_LEVEL_FILTER` controls the verbosity. 
You can change it by setting it at compile time to a string matching the name of a [LevelFilter](https://docs.rs/log/0.4.8/log/enum.LevelFilter.html).
If the variable is not set, or the name doesn't match, then `LevelFilter::Info` is used by default.

```sh
$ HERMIT_LOG_LEVEL_FILTER=Debug cargo xtask build --arch x86_64
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
