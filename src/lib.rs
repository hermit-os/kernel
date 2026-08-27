//! The Hermit kernel.
//!
//! This _library operating system_ (libOS) compiles to a static library
//! (libhermit.a) that applications can link against to create a _Unikernel_.
//!
//! The API documented here does not matter to such an application.
//! Such an application would use it's languages standard library which
//! internally calls this kernel's system call functions ([`syscalls`]).
//!
//! # Using Hermit
//!
//! To run a Rust application with Hermit, see [hermit-rs].
//!
//! To run a C or C++ application with Hermit, see [hermit-c].
//!
//! # Building the kernel manually
//!
//! You can build the kernel with default features for x86-64 like this:
//!
//! ```sh
//! cargo xtask build --arch x86_64
//! ```
//!
//! For more information, run:
//!
//! ```
//! cargo xtask build --help
//! ```
//!
//! # Features
//!
#![cfg_attr(
	not(feature = "document-features"),
	doc = "Activate the `document-features` Cargo feature to see feature docs here."
)]
#![cfg_attr(feature = "document-features", doc = document_features::document_features!())]
//!
//! [hermit-rs]: https://github.com/hermit-os/hermit-rs
//! [hermit-c]: https://github.com/hermit-os/hermit-c

#![allow(clippy::missing_safety_doc)]
#![cfg_attr(
	any(target_arch = "aarch64", target_arch = "riscv64"),
	allow(incomplete_features)
)]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![feature(allocator_api)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(
	all(
		not(any(feature = "common-os", feature = "nostd")),
		not(target_arch = "riscv64"),
	),
	feature(linkage)
)]
#![feature(linked_list_cursors)]
#![feature(never_type)]
#![cfg_attr(
	any(target_arch = "aarch64", target_arch = "riscv64"),
	feature(specialization)
)]
#![cfg_attr(
	all(
		not(any(feature = "common-os", feature = "nostd")),
		not(target_arch = "riscv64"),
	),
	feature(thread_local)
)]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(custom_test_frameworks))]
#![cfg_attr(all(target_os = "none", test), test_runner(crate::rt::test_runner))]
#![cfg_attr(
	all(target_os = "none", test),
	reexport_test_harness_main = "test_main"
)]
#![cfg_attr(all(target_os = "none", test), no_main)]
// FIXME: move this to `Cargo.toml` once stable
#![feature(strict_provenance_lints)]
#![warn(implicit_provenance_casts)]

#[cfg(all(feature = "snapshot", feature = "write-pcap-file", not(doc)))]
compile_error!("The `snapshot` feature is incompatible with the `write-pcap-file` feature.");

// EXTERNAL CRATES
#[macro_use]
extern crate alloc;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate log;
#[cfg(not(target_os = "none"))]
#[macro_use]
extern crate std;

#[macro_use]
mod macros;

#[macro_use]
mod logging;

pub mod arch;
#[cfg(all(feature = "common-os", target_arch = "x86_64"))]
pub mod common_os;
pub mod config;
pub mod console;
mod drivers;
mod entropy;
mod env;
pub mod errno;
mod executor;
pub mod fd;
pub mod fs;
mod init_buf;
mod init_cell;
pub mod io;
pub mod mm;
mod page_range_ext;
#[cfg(target_os = "none")]
pub mod rt;
pub mod scheduler;
#[cfg(feature = "shell")]
mod shell;
mod synch;
pub mod syscalls;
pub mod time;
#[cfg(feature = "uhyve")]
mod uhyve;
