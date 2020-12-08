// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::irq;
use core::cell::UnsafeCell;
use core::fmt;
use core::marker::Sync;
use core::ops::{Deref, DerefMut, Drop};
use core::sync::atomic::{spin_loop_hint, AtomicBool, AtomicUsize, Ordering};
use crossbeam_utils::CachePadded;

/// This type provides a lock based on busy waiting to realize mutual exclusion of tasks.
///
/// # Description
///
/// This structure behaves a lot like a normal Mutex. There are some differences:
///
/// - By using busy waiting, it can be used outside the runtime.
/// - It is a so called ticket lock (https://en.wikipedia.org/wiki/Ticket_lock)
///   and completly fair.
///
/// The interface is derived from https://mvdnes.github.io/rust-docs/spin-rs/spin/index.html.
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
pub struct Spinlock<T: ?Sized> {
	queue: CachePadded<AtomicUsize>,
	dequeue: CachePadded<AtomicUsize>,
	data: UnsafeCell<T>,
}

/// A guard to which the protected data can be accessed
///
/// When the guard falls out of scope it will release the lock.
pub struct SpinlockGuard<'a, T: ?Sized + 'a> {
	//queue: &'a AtomicUsize,
	dequeue: &'a CachePadded<AtomicUsize>,
	data: &'a mut T,
}

// Same unsafe impls as `Spinlock`
unsafe impl<T: ?Sized + Send> Sync for Spinlock<T> {}
unsafe impl<T: ?Sized + Send> Send for Spinlock<T> {}

impl<T> Spinlock<T> {
	pub const fn new(user_data: T) -> Spinlock<T> {
		Spinlock {
			queue: CachePadded::new(AtomicUsize::new(0)),
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
	fn obtain_lock(&self) {
		let ticket = self.queue.fetch_add(1, Ordering::SeqCst) + 1;
		while self.dequeue.load(Ordering::SeqCst) != ticket {
			spin_loop_hint();
		}
	}

	pub fn lock(&self) -> SpinlockGuard<T> {
		self.obtain_lock();
		SpinlockGuard {
			//queue: &self.queue,
			dequeue: &self.dequeue,
			data: unsafe { &mut *self.data.get() },
		}
	}
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Spinlock<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "queue: {} ", self.queue.load(Ordering::SeqCst))?;
		write!(f, "dequeue: {}", self.dequeue.load(Ordering::SeqCst))
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

impl<'a, T: ?Sized> Drop for SpinlockGuard<'a, T> {
	/// The dropping of the SpinlockGuard will release the lock it was created from.
	fn drop(&mut self) {
		self.dequeue.fetch_add(1, Ordering::SeqCst);
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
/// - It is a so called ticket lock (https://en.wikipedia.org/wiki/Ticket_lock)
///   and completly fair.
///
/// The interface is derived from https://mvdnes.github.io/rust-docs/spin-rs/spin/index.html.
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
	queue: CachePadded<AtomicUsize>,
	dequeue: CachePadded<AtomicUsize>,
	irq: AtomicBool,
	data: UnsafeCell<T>,
}

/// A guard to which the protected data can be accessed
///
/// When the guard falls out of scope it will release the lock.
pub struct SpinlockIrqSaveGuard<'a, T: ?Sized + 'a> {
	//queue: &'a AtomicUsize,
	dequeue: &'a CachePadded<AtomicUsize>,
	irq: &'a AtomicBool,
	data: &'a mut T,
}

// Same unsafe impls as `SoinlockIrqSave`
unsafe impl<T: ?Sized + Send> Sync for SpinlockIrqSave<T> {}
unsafe impl<T: ?Sized + Send> Send for SpinlockIrqSave<T> {}

impl<T> SpinlockIrqSave<T> {
	pub const fn new(user_data: T) -> SpinlockIrqSave<T> {
		SpinlockIrqSave {
			queue: CachePadded::new(AtomicUsize::new(0)),
			dequeue: CachePadded::new(AtomicUsize::new(1)),
			irq: AtomicBool::new(false),
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
	fn obtain_lock(&self) {
		let irq = irq::nested_disable();

		let ticket = self.queue.fetch_add(1, Ordering::SeqCst) + 1;
		while self.dequeue.load(Ordering::SeqCst) != ticket {
			spin_loop_hint();
		}

		self.irq.store(irq, Ordering::SeqCst);
	}

	pub fn lock(&self) -> SpinlockIrqSaveGuard<T> {
		self.obtain_lock();
		SpinlockIrqSaveGuard {
			//queue: &self.queue,
			dequeue: &self.dequeue,
			irq: &self.irq,
			data: unsafe { &mut *self.data.get() },
		}
	}
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for SpinlockIrqSave<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "irq: {:?} ", self.irq)?;
		write!(f, "queue: {} ", self.queue.load(Ordering::SeqCst))?;
		write!(f, "dequeue: {}", self.dequeue.load(Ordering::SeqCst))
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
		let irq = self.irq.swap(false, Ordering::SeqCst);
		self.dequeue.fetch_add(1, Ordering::SeqCst);
		irq::nested_enable(irq);
	}
}
