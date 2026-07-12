//! Architecture-specific architecture abstraction.

// FIXME: use cfg_select! instead once resolved:
// https://github.com/rust-lang/rust/issues/158371
// https://github.com/rust-lang/rust/issues/158400
#[cfg(target_arch = "aarch64")]
mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use self::aarch64::mm::clear_user_space;
#[cfg(target_arch = "aarch64")]
pub use self::aarch64::mm::paging::{BasePageSize, PageSize};
#[cfg(target_arch = "aarch64")]
pub(crate) use self::aarch64::*;

#[cfg(target_arch = "riscv64")]
mod riscv64;
#[cfg(target_arch = "riscv64")]
pub(crate) use self::riscv64::*;

#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use self::x86_64::mm::{BasePageSize, PageSize, clear_user_space};
#[cfg(target_arch = "x86_64")]
pub(crate) use self::x86_64::*;
