use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossbeam_utils::Backoff;
use lock_api::{GuardSend, Mutex, MutexGuard, RawMutex};

use crate::arch::irq;

/// Based on `spin::mutex::TicketMutex`, but with backoff.
pub struct RawTicketMutex {
	next_ticket: AtomicUsize,
	next_serving: AtomicUsize,
}

unsafe impl RawMutex for RawTicketMutex {
	#[allow(clippy::declare_interior_mutable_const)]
	const INIT: Self = Self {
		next_ticket: AtomicUsize::new(0),
		next_serving: AtomicUsize::new(0),
	};

	type GuardMarker = GuardSend;

	#[inline]
	fn lock(&self) {
		let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);

		let backoff = Backoff::new();
		while self.next_serving.load(Ordering::Acquire) != ticket {
			backoff.spin();
		}
	}

	#[inline]
	fn try_lock(&self) -> bool {
		let ticket = self
			.next_ticket
			.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |ticket| {
				if self.next_serving.load(Ordering::Acquire) == ticket {
					Some(ticket + 1)
				} else {
					None
				}
			});

		ticket.is_ok()
	}

	#[inline]
	unsafe fn unlock(&self) {
		self.next_serving.fetch_add(1, Ordering::Release);
	}

	#[inline]
	fn is_locked(&self) -> bool {
		let ticket = self.next_ticket.load(Ordering::Relaxed);
		self.next_serving.load(Ordering::Relaxed) != ticket
	}
}

pub type Spinlock<T> = Mutex<RawTicketMutex, T>;
pub type SpinlockGuard<'a, T> = MutexGuard<'a, RawTicketMutex, T>;

/// An interrupt-safe mutex.
pub struct RawInterruptMutex<M> {
	inner: M,
	interrupts: AtomicBool,
}

unsafe impl<M: RawMutex> RawMutex for RawInterruptMutex<M> {
	const INIT: Self = Self {
		inner: M::INIT,
		interrupts: AtomicBool::new(false),
	};

	type GuardMarker = M::GuardMarker;

	#[inline]
	fn lock(&self) {
		let interrupts = irq::nested_disable();
		self.inner.lock();
		self.interrupts.store(interrupts, Ordering::Relaxed);
	}

	#[inline]
	fn try_lock(&self) -> bool {
		let interrupts = irq::nested_disable();
		let ok = self.inner.try_lock();
		if !ok {
			irq::nested_enable(interrupts);
		}
		ok
	}

	#[inline]
	unsafe fn unlock(&self) {
		let interrupts = self.interrupts.swap(false, Ordering::Relaxed);
		unsafe {
			self.inner.unlock();
		}
		irq::nested_enable(interrupts);
	}

	#[inline]
	fn is_locked(&self) -> bool {
		self.inner.is_locked()
	}
}

type RawInterruptTicketMutex = RawInterruptMutex<RawTicketMutex>;
pub type SpinlockIrqSave<T> = Mutex<RawInterruptTicketMutex, T>;
pub type SpinlockIrqSaveGuard<'a, T> = MutexGuard<'a, RawInterruptTicketMutex, T>;
