<img width="100" align="right" src="img/hermitcore_logo.png" />

# RustyHermit: libhermit-rs

[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg)](https://hermitcore.github.io/libhermit-rs/hermit/)
[![License](https://img.shields.io/crates/l/rusty-hermit.svg)](https://img.shields.io/crates/l/rusty-hermit.svg)
[![Slack Status](https://matrix.osbyexample.com:3008/badge.svg)](https://matrix.osbyexample.com:3008)

_libhermit-rs_ is the kernel of the [RustyHermit](https://github.com/hermitcore/rusty-hermit) unikernel project.

## Building the kernel

Usually the kernel will be linked as static library to your applications.

- **Rust applications:** Instructions can be found in the [rusty-hermit](https://github.com/hermitcore/rusty-hermit) repository.
- **For C/C++ applications:** Instructions can be found in the [hermit-playground](https://github.com/hermitcore/hermit-playground) repository.
 

### Standalone static library build

If this does not fit your needs and you want to build the kernel as static library to link afterwards, you need the following:

The Rust **nightly** toolchain ([official webpage](https://www.rust-lang.org/)), the source code of the Rust runtime, and llvm-tools:

```sh
rustup toolchain install nightly
rustup component add rust-src llvm-tools-preview
```

You can then build `libhermit-rs` with the following command

```sh
cargo build -Z build-std=core,alloc,panic_abort --target x86_64-unknown-hermit-kernel
```

The resulting library then can be found in `target/x86_64-unknown-hermit-kernel/debug/libhermit.a`


### Control the kernel messages verbosity

_libhermit-rs_ uses the lightweight logging crate [log](https://github.com/rust-lang/log) to print kernel messages.
The environment variable `HERMIT_LOG_LEVEL_FILTER` controls the verbosity. 
You can change it by setting it at compile time to a string matching the name of a [LevelFilter](https://docs.rs/log/0.4.8/log/enum.LevelFilter.html).
If the variable is not set, or the name doesn't match, then `LevelFilter::Info` is used by default.

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
