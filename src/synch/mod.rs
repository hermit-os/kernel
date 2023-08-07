//! Synchronization primitives

pub mod futex;
#[cfg(feature = "newlib")]
pub mod recmutex;
pub mod semaphore;
