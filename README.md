<img width="100" align="right" src="img/hermitcore_logo.png" />

# libhermit-rs - A Rust-based library operting system

[![Build Status](https://travis-ci.com/hermitcore/libhermit-rs.svg?branch=master)](https://travis-ci.com/hermitcore/libhermit-rs)
![Actions Status](https://github.com/hermitcore/libhermit-rs/workflows/Build/badge.svg)
[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg)](https://hermitcore.github.io/libhermit-rs/hermit/)
[![License](https://img.shields.io/crates/l/rusty-hermit.svg)](https://img.shields.io/crates/l/rusty-hermit.svg)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

[RustyHermit](https://github.com/hermitcore/rusty-hermit) is a [unikernel](http://unikernel.org) targeting a scalable and predictable runtime for high-performance and cloud computing.
Unikernel means, you bundle your application directly with the kernel library, so that it can run without any installed operating system.
This reduces overhead, therefore, interesting applications include virtual machines and high-performance computing.

_libhermit-rs_ is the heart of RustyHermit and is the kernel itself.
The kernel is able to run [Rust](https://github.com/hermitcore/hermit-playground) applications, as well as [C/C++/Go/Fortran](https://github.com/hermitcore/rusty-hermit) applications.

## Prerequisites

The Rust toolchain can be installed from the [official webpage](https://www.rust-lang.org/).
RusyHermit currently requires the **nightly versions** of the toolchain.
```sh
rustup default nightly
```

Further requirements are the source code of the Rust runtime, and llvm-tools:

```sh
rustup component add rust-src
rustup component add llvm-tools-preview
```

## Building the kernel as static library

The kernel will be linked as static library to C/C++ or Rust applications.
In case of Rust, the crate [hermit-sys](https://github.com/hermitcore/rusty-hermit) automate this process.
For C/C++ applications a modified [C/C++ compiler](https://github.com/hermitcore/hermit-playground) has to be used.
To build the kernel as static library and to link afterwards by its own to the applicatiom, please use following build command:

```sh
cargo build -Z build-std=core,alloc,panic_abort --target x86_64-unknown-hermit-kernel
```

The resulting library then can be found in `target/x86_64-unknown-hermit-kernel/debug/`


## Controlling the number of kernel messages

_libhermit-rs_ uses the lightweight logging crate [log](https://github.com/rust-lang/log) to print kernel messages.
If the environment variable `HERMIT_LOG_LEVEL_FILTER` is set at compile time to a string matching the name of a [LevelFilter](https://docs.rs/log/0.4.8/log/enum.LevelFilter.html), then that value is used for the LevelFilter.
If the environment variable is not set, or the name doesn't match, then LevelFilter::Info is used by default, which is the same as it was before.

For instance, the following command build RustyHermit with debug messages:

```sh
$ HERMIT_LOG_LEVEL_FILTER=Debug cargo build -Z build-std=core,alloc,panic_abort --target x86_64-unknown-hermit-kernel
```


## Credits

_libhermit-rs_ is derived from following tutorials and software distributions:

1. Philipp Oppermann's [excellent series of blog posts][opp].
2. Erik Kidd's [toyos-rs][kidd], which is an extension of Philipp Opermann's kernel.
3. The Rust-based teaching operating system [eduOS-rs][eduos].

[opp]: http://blog.phil-opp.com/
[kidd]: http://www.randomhacks.net/bare-metal-rust/
[eduos]: http://rwth-os.github.io/eduOS-rs/

HermitCore's Emoji is provided for free by [EmojiOne](https://www.gfxmag.com/crab-emoji-vector-icon/).

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

libhermit-rs is being developed on [GitHub](https://github.com/hermitcore/libhermit-rs).
Create your own fork, send us a pull request, and chat with us on [Slack](https://radiant-ridge-95061.herokuapp.com)
