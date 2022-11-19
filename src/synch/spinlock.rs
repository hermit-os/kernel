use hermit_sync::{InterruptMutex, InterruptMutexGuard, RawTicketMutex};
pub use hermit_sync::{TicketMutex as Spinlock, TicketMutexGuard as SpinlockGuard};

pub type SpinlockIrqSave<T> = InterruptMutex<RawTicketMutex, T>;
pub type SpinlockIrqSaveGuard<'a, T> = InterruptMutexGuard<'a, RawTicketMutex, T>;
