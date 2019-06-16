<img width="100" align="right" src="img/hermitcore_logo.png" />


# HermitCore-rs - A Rust-based, lightweight unikernel for a scalable and predictable runtime behavior

[![Build Status](https://travis-ci.org/hermitcore/libhermit-rs.svg?branch=master)](https://travis-ci.org/hermitcore/libhermit-rs)
[![Slack Status](https://radiant-ridge-95061.herokuapp.com/badge.svg)](https://radiant-ridge-95061.herokuapp.com)

[HermitCore]( http://www.hermitcore.org ) is a new
[unikernel](http://unikernel.org) targeting a scalable and predictable runtime
for high-performance and cloud computing. HermitCore extends the multi-kernel
approach (like
[McKernel](https://www-sys-aics.riken.jp/ResearchTopics/os/mckernel/)) with
unikernel features for a better programmability and scalability for hierarchical
systems.

We decided to develop a version of the kernel in [Rust](https://www.rust-lang.org) called *HermitCore-rs*.
We promise that this will make it easier to maintain and extend our kernel.
All code beside the kernel can still be developed in your preferred language (C/C++/Go/Fortran).

This repository contains only the Rust-based kernel of HermitCore.
The complete toolchain and a few demo applications are published at [https://github.com/hermitcore/hermit-playground](https://github.com/hermitcore/hermit-playground).
Currently, the Rust-based version does not support all features of the [C-based version](https://github.com/hermitcore/libhermit).
However, it is a starting point and runs within a hypervisor.
The multi-kernel approach has not yet been tested in it.

## Credits

HermitCore's Emoji is provided for free by [EmojiOne](https://www.gfxmag.com/crab-emoji-vector-icon/).

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

HermitCore-rs is being developed on [GitHub](https://github.com/hermitcore/libhermit-rs).
Create your own fork, send us a pull request, and chat with us on [Slack](https://radiant-ridge-95061.herokuapp.com)
