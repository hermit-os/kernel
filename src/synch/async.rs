#![allow(dead_code)]

use core::cell::UnsafeCell;
use core::future;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;

use crossbeam_utils::Backoff;

#[derive(Debug)]
pub(crate) struct AsyncInterruptMutex<T: ?Sized> {
	lock: AtomicBool,
	interrupt_guard: UnsafeCell<MaybeUninit<interrupts::Guard>>,
	data: UnsafeCell<T>,
}

/// A guard to which the protected data can be accessed
///
/// When the guard falls out of scope it will release the lock.
#[derive(Debug)]
pub(crate) struct AsyncInterruptMutexGuard<'a, T: ?Sized> {
	lock: &'a AtomicBool,
	interrupt_guard: &'a UnsafeCell<MaybeUninit<interrupts::Guard>>,
	data: &'a mut T,
}

unsafe impl<T: ?Sized + Send> Sync for AsyncInterruptMutex<T> {}
unsafe impl<T: ?Sized + Send> Send for AsyncInterruptMutex<T> {}

impl<T> AsyncInterruptMutex<T> {
	pub const fn new(data: T) -> AsyncInterruptMutex<T> {
		Self {
			lock: AtomicBool::new(false),
			interrupt_guard: UnsafeCell::new(MaybeUninit::uninit()),
			data: UnsafeCell::new(data),
		}
	}

	#[inline]
	fn obtain_lock(&self) {
		let backoff = Backoff::new();
		let guard = interrupts::disable();
		while self
			.lock
			.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
			.is_err()
		{
			while self.is_locked() {
				backoff.snooze();
			}
		}
		// SAFETY: We have exclusive access through locking `inner`.
		unsafe {
			self.interrupt_guard.get().write(MaybeUninit::new(guard));
		}
	}

	#[inline]
	fn is_locked(&self) -> bool {
		self.lock.load(Ordering::Relaxed)
	}

	pub fn lock(&self) -> AsyncInterruptMutexGuard<'_, T> {
		self.obtain_lock();
		AsyncInterruptMutexGuard {
			lock: &self.lock,
			interrupt_guard: &self.interrupt_guard,
			data: unsafe { &mut *self.data.get() },
		}
	}

	#[inline]
	fn obtain_try_lock(&self) -> bool {
		let guard = interrupts::disable();
		let ok = self
			.lock
			.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
			.is_ok();
		if ok {
			// SAFETY: We have exclusive access through locking `inner`.
			unsafe {
				self.interrupt_guard.get().write(MaybeUninit::new(guard));
			}
		}

		ok
	}

	pub async fn async_lock(&self) -> AsyncInterruptMutexGuard<'_, T> {
		future::poll_fn(|cx| {
			if self.obtain_try_lock() {
				Poll::Ready(AsyncInterruptMutexGuard {
					lock: &self.lock,
					interrupt_guard: &self.interrupt_guard,
					data: unsafe { &mut *self.data.get() },
				})
			} else {
				cx.waker().wake_by_ref();
				Poll::Pending
			}
		})
		.await
	}
}

impl<'a, T: ?Sized> Deref for AsyncInterruptMutexGuard<'a, T> {
	type Target = T;
	fn deref(&self) -> &T {
		&*self.data
	}
}

impl<'a, T: ?Sized> DerefMut for AsyncInterruptMutexGuard<'a, T> {
	fn deref_mut(&mut self) -> &mut T {
		&mut *self.data
	}
}

impl<'a, T: ?Sized> Drop for AsyncInterruptMutexGuard<'a, T> {
	/// The dropping of the AsyncInterruptMutexGuard will release the lock it was created from.
	fn drop(&mut self) {
		// SAFETY: We have exclusive access through locking `inner`.
		let guard = unsafe { self.interrupt_guard.get().replace(MaybeUninit::uninit()) };
		// SAFETY: `guard` was initialized when locking.
		let _guard = unsafe { guard.assume_init() };
		self.lock.store(false, Ordering::Release);
	}
}
