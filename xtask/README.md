# Hermit kernel's xtask utility

This program is primarily used for integration tests.

## initramfs support

In order to facilitate testing of `initramfs` / Hermit Images, this tool's
`cargo xtask ci rs` subcommand supports automatic creation of such compressed tarballs.
This is done automatically if an examples' directory contains a folder named `initramfs`.
It is expected that each such folder contains an `hermit.toml` file following the
specification in [`hermit_entry::config::Config`](https://docs.rs/hermit-entry/latest/hermit_entry/config/enum.Config.html).
