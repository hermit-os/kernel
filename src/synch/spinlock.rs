#![allow(dead_code)]

use crate::arch::irq;
use core::cell::UnsafeCell;
#[cfg(feature = "smp")]
use core::fmt;
use core::marker::Sync;
use core::ops::{Deref, DerefMut, Drop};
#[cfg(feature = "smp")]
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "smp")]
use crossbeam_utils::{Backoff, CachePadded};

/// This type provides a lock based on busy waiting to realize mutual exclusion of tasks.
///
/// # Description
///
/// This structure behaves a lot like a normal Mutex. There are some differences:
///
/// - By using busy waiting, it can be used outside the runtime.
/// - It is a so called ticket lock (<https://en.wikipedia.org/wiki/Ticket_lock>)
///   and completely fair.
///
/// The interface is derived from <https://mvdnes.github.io/rust-docs/spin-rs/spin/index.html>.
///
/// # Simple examples
///
/// ```
/// let spinlock = synch::Spinlock::new(0);
///
/// // Modify the data
/// {
///     let mut data = spinlock.lock();
///     *data = 2;
/// }
///
/// // Read the data
/// let answer =
/// {
///     let data = spinlock.lock();
///     *data
/// };
///
/// assert_eq!(answer, 2);
/// ```
#[cfg(feature = "smp")]
pub struct Spinlock<T: ?Sized> {
	queue: CachePadded<AtomicUsize>,
	dequeue: CachePadded<AtomicUsize>,
	data: UnsafeCell<T>,
}

#[cfg(not(feature = "smp"))]
pub struct Spinlock<T: ?Sized> {
	data: UnsafeCell<T>,
}

/// A guard to which the protected data can be accessed
///
/// When the guard falls out of scope it will release the lock.
pub struct SpinlockGuard<'a, T: ?Sized> {
	#[cfg(feature = "smp")]
	dequeue: &'a CachePadded<AtomicUsize>,
	#[cfg(feature = "smp")]
	ticket: usize,
	data: &'a mut T,
}

// Same unsafe impls as `Spinlock`
unsafe impl<T: ?Sized + Send> Sync for Spinlock<T> {}
unsafe impl<T: ?Sized + Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
	pub const fn new(user_data: T) -> Spinlock<T> {
		Spinlock {
			#[cfg(feature = "smp")]
			queue: CachePadded::new(AtomicUsize::new(0)),
			#[cfg(feature = "smp")]
			dequeue: CachePadded::new(AtomicUsize::new(1)),
			data: UnsafeCell::new(user_data),
		}
	}

	/// Consumes this mutex, returning the underlying data.
	#[allow(dead_code)]
	pub fn into_inner(self) -> T {
		// We know statically that there are no outstanding references to
		// `self` so there's no need to lock.
		let Spinlock { data, .. } = self;
		data.into_inner()
	}
}

impl<T: ?Sized> Spinlock<T> {
	#[cfg(feature = "smp")]
	pub fn lock(&self) -> SpinlockGuard<'_, T> {
		let backoff = Backoff::new();
		let ticket = self.queue.fetch_add(1, Ordering::Relaxed) + 1;

		while self.dequeue.load(Ordering::Acquire) != ticket {
			backoff.spin();
		}

		SpinlockGuard {
			dequeue: &self.dequeue,
			ticket,
			data: unsafe { &mut *self.data.get() },
		}
	}

	#[cfg(not(feature = "smp"))]
	pub fn lock(&self) -> SpinlockGuard<'_, T> {
		SpinlockGuard {
			data: unsafe { &mut *self.data.get() },
		}
	}

	#[cfg(feature = "smp")]
	pub fn try_lock(&self) -> Result<SpinlockGuard<'_, T>, ()> {
		self.queue
			.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |ticket| {
				if self.dequeue.load(Ordering::Acquire) == ticket + 1 {
					Some(ticket + 1)
				} else {
					None
				}
			})
			.map(|ticket| SpinlockGuard {
				dequeue: &self.dequeue,
				ticket,
				data: unsafe { &mut *self.data.get() },
			})
			.map_err(|_| {})
	}

	#[cfg(not(feature = "smp"))]
	pub fn try_lock(&self) -> Result<SpinlockGuard<'_, T>, ()> {
		Ok(SpinlockGuard {
			data: unsafe { &mut *self.data.get() },
		})
	}
}

#[cfg(feature = "smp")]
impl<T: ?Sized + fmt::Debug> fmt::Debug for Spinlock<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "queue: {} ", self.queue.load(Ordering::Relaxed))?;
		write!(f, "dequeue: {}", self.dequeue.load(Ordering::Relaxed))
	}
}

impl<T: ?Sized + Default> Default for Spinlock<T> {
	fn default() -> Spinlock<T> {
		Spinlock::new(Default::default())
	}
}

impl<'a, T: ?Sized> Deref for SpinlockGuard<'a, T> {
	type Target = T;
	fn deref(&self) -> &T {
		&*self.data
	}
}

impl<'a, T: ?Sized> DerefMut for SpinlockGuard<'a, T> {
	fn deref_mut(&mut self) -> &mut T {
		&mut *self.data
	}
}

#[cfg(feature = "smp")]
impl<'a, T: ?Sized> Drop for SpinlockGuard<'a, T> {
	/// The dropping of the SpinlockGuard will release the lock it was created from.
	fn drop(&mut self) {
		self.dequeue.store(self.ticket + 1, Ordering::Release);
	}
}

/// This type provides a lock based on busy waiting to realize mutual exclusion of tasks.
///
/// # Description
///
/// This structure behaves a lot like a normal Mutex. There are some differences:
///
/// - Interrupts save lock => Interrupts will be disabled
/// - By using busy waiting, it can be used outside the runtime.
/// - It is a so called ticket lock (<https://en.wikipedia.org/wiki/Ticket_lock>)
///   and completely fair.
///
/// The interface is derived from <https://mvdnes.github.io/rust-docs/spin-rs/spin/index.html>.
///
/// # Simple examples
///
/// ```
/// let spinlock = synch::SpinlockIrqSave::new(0);
///
/// // Modify the data
/// {
///     let mut data = spinlock.lock();
///     *data = 2;
/// }
///
/// // Read the data
/// let answer =
/// {
///     let data = spinlock.lock();
///     *data
/// };
///
/// assert_eq!(answer, 2);
/// ```
pub struct SpinlockIrqSave<T: ?Sized> {
	#[cfg(feature = "smp")]
	queue: CachePadded<AtomicUsize>,
	#[cfg(feature = "smp")]
	dequeue: CachePadded<AtomicUsize>,
	data: UnsafeCell<T>,
}

/// A guard to which the protected data can be accessed
///
/// When the guard falls out of scope it will release the lock.
pub struct SpinlockIrqSaveGuard<'a, T: ?Sized> {
	#[cfg(feature = "smp")]
	dequeue: &'a CachePadded<AtomicUsize>,
	#[cfg(feature = "smp")]
	ticket: usize,
	irq: bool,
	data: &'a mut T,
}

// Same unsafe impls as `SoinlockIrqSave`
unsafe impl<T: ?Sized + Send> Sync for SpinlockIrqSave<T> {}
unsafe impl<T: ?Sized + Send> Send for SpinlockIrqSave<T> {}

impl<T> SpinlockIrqSave<T> {
	pub const fn new(user_data: T) -> SpinlockIrqSave<T> {
		SpinlockIrqSave {
			#[cfg(feature = "smp")]
			queue: CachePadded::new(AtomicUsize::new(0)),
			#[cfg(feature = "smp")]
			dequeue: CachePadded::new(AtomicUsize::new(1)),
			data: UnsafeCell::new(user_data),
		}
	}

	/// Consumes this mutex, returning the underlying data.
	#[allow(dead_code)]
	pub fn into_inner(self) -> T {
		// We know statically that there are no outstanding references to
		// `self` so there's no need to lock.
		let SpinlockIrqSave { data, .. } = self;
		data.into_inner()
	}
}

impl<T: ?Sized> SpinlockIrqSave<T> {
	#[cfg(not(feature = "smp"))]
	pub fn try_lock(&self) -> Result<SpinlockIrqSaveGuard<'_, T>, ()> {
		let irq = unsafe { irq::nested_disable() };
		if irq {
			Ok(SpinlockIrqSaveGuard {
				irq,
				data: unsafe { &mut *self.data.get() },
			})
		} else {
			Err(())
		}
	}

	#[cfg(feature = "smp")]
	pub fn try_lock(&self) -> Result<SpinlockIrqSaveGuard<'_, T>, ()> {
		let irq = irq::nested_disable();
		self.queue
			.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |ticket| {
				if self.dequeue.load(Ordering::Acquire) == ticket + 1 {
					Some(ticket + 1)
				} else {
					None
				}
			})
			.map(|ticket| SpinlockIrqSaveGuard {
				dequeue: &self.dequeue,
				ticket: ticket + 1,
				irq,
				data: unsafe { &mut *self.data.get() },
			})
			.map_err(|_| {})
	}

	#[cfg(not(feature = "smp"))]
	pub fn lock(&self) -> SpinlockIrqSaveGuard<'_, T> {
		let irq = irq::nested_disable();
		SpinlockIrqSaveGuard {
			irq,
			data: unsafe { &mut *self.data.get() },
		}
	}

	#[cfg(feature = "smp")]
	pub fn lock(&self) -> SpinlockIrqSaveGuard<'_, T> {
		let irq = irq::nested_disable();
		let backoff = Backoff::new();
		let ticket = self.queue.fetch_add(1, Ordering::Relaxed) + 1;

		while self.dequeue.load(Ordering::Acquire) != ticket {
			backoff.spin();
		}

		SpinlockIrqSaveGuard {
			dequeue: &self.dequeue,
			ticket,
			irq,
			data: unsafe { &mut *self.data.get() },
		}
	}
}

#[cfg(feature = "smp")]
impl<T: ?Sized + fmt::Debug> fmt::Debug for SpinlockIrqSave<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "queue: {} ", self.queue.load(Ordering::Relaxed))?;
		write!(f, "dequeue: {}", self.dequeue.load(Ordering::Relaxed))
	}
}

impl<T: ?Sized + Default> Default for SpinlockIrqSave<T> {
	fn default() -> SpinlockIrqSave<T> {
		SpinlockIrqSave::new(Default::default())
	}
}

impl<'a, T: ?Sized> Deref for SpinlockIrqSaveGuard<'a, T> {
	type Target = T;
	fn deref(&self) -> &T {
		&*self.data
	}
}

impl<'a, T: ?Sized> DerefMut for SpinlockIrqSaveGuard<'a, T> {
	fn deref_mut(&mut self) -> &mut T {
		&mut *self.data
	}
}

impl<'a, T: ?Sized> Drop for SpinlockIrqSaveGuard<'a, T> {
	/// The dropping of the SpinlockGuard will release the lock it was created from.
	fn drop(&mut self) {
		#[cfg(feature = "smp")]
		self.dequeue.store(self.ticket + 1, Ordering::Release);
		irq::nested_enable(self.irq);
	}
}
