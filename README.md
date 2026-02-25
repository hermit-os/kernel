# Hermit Kernel

[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg)](https://hermit-os.github.io/kernel)
![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)
[![Zulip Badge](https://img.shields.io/badge/chat-hermit-57A37C?logo=zulip)](https://hermit.zulipchat.com/)
[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.14645534.svg)](https://doi.org/10.5281/zenodo.14645534)

This is the kernel of the [Hermit](https://github.com/hermit-os) unikernel project.

For details, see the [docs].

This crate is no longer distributed via crates.io.
To upgrade, use the crate via Git instead:

```diff
-hermit-kernel = "0.11"
+hermit-kernel = { git = "https://github.com/hermit-os/kernel.git", tag = "v0.13.0" }
```

[docs]: https://hermit-os.github.io/kernel
