//! Synchronization primitives

pub(crate) mod r#async;
pub mod futex;
#[cfg(feature = "newlib")]
pub mod recmutex;
pub mod semaphore;
