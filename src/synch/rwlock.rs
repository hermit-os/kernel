//! This library provides a phase-fair reader-writer lock, as described in the
//! paper ["Reader-Writer Synchronization for Shared-Memory Multiprocessor
//! Real-Time Systems"](https://www.cs.unc.edu/~anderson/papers/ecrts09b.pdf)
//! by Brandenburg et. al.
//!
//! > Reader preference, writer preference, and task-fair reader-writer locks are
//! > shown to cause undue blocking in multiprocessor real-time systems. A new
//! > phase-fair reader-writer lock is proposed as an alternative that
//! > significantly reduces worst-case blocking for readers.
//!
//! This implementation is derived from <https://github.com/cmnord/pflock> and
//! modified for RustyHermit.

#![allow(dead_code)]

use core::hint::spin_loop;
use core::sync::atomic::{AtomicUsize, Ordering};

use lock_api::{GuardSend, RawRwLock, RwLock};

pub(crate) struct RWSpinLock {
	rin: AtomicUsize,
	rout: AtomicUsize,
	win: AtomicUsize,
	wout: AtomicUsize,
}

const RINC: usize = 0x100; // reader increment
const WBITS: usize = 0x3; // writer bits in rin
const PRES: usize = 0x2; // writer present bit
const PHID: usize = 0x1; // phase ID bit
const ZERO_MASK: usize = !255usize;

unsafe impl RawRwLock for RWSpinLock {
	#[allow(clippy::declare_interior_mutable_const)]
	const INIT: RWSpinLock = RWSpinLock {
		rin: AtomicUsize::new(0),
		rout: AtomicUsize::new(0),
		win: AtomicUsize::new(0),
		wout: AtomicUsize::new(0),
	};

	type GuardMarker = GuardSend;

	#[inline]
	fn lock_shared(&self) {
		// Increment the rin count and read the writer bits
		let w = self.rin.fetch_add(RINC, Ordering::Relaxed) & WBITS;

		// Spin (wait) if there is a writer present (w != 0), until either PRES
		// and/or PHID flips
		while (w != 0) && (w == (self.rin.load(Ordering::Relaxed) & WBITS)) {
			spin_loop();
		}
	}

	#[inline]
	unsafe fn unlock_shared(&self) {
		// Increment rout to mark the read-lock returned
		self.rout.fetch_add(RINC, Ordering::Relaxed);
	}

	#[inline]
	fn try_lock_shared(&self) -> bool {
		let w = self.rin.fetch_add(RINC, Ordering::Relaxed) & WBITS;

		if w == 0 || w != (self.rin.load(Ordering::Relaxed) & WBITS) {
			true
		} else {
			self.rout.fetch_add(RINC, Ordering::Relaxed);
			false
		}
	}

	#[inline]
	fn lock_exclusive(&self) {
		// Wait until it is my turn to write-lock the resource
		let wticket = self.win.fetch_add(1, Ordering::Relaxed);
		while wticket != self.wout.load(Ordering::Relaxed) {
			spin_loop();
		}

		// Set the write-bits of rin to indicate this writer is here
		let w = PRES | (wticket & PHID);
		let rticket = self.rin.fetch_add(w, Ordering::Relaxed);

		// Wait until all current readers have finished (i.e. rout catches up)
		while rticket != self.rout.load(Ordering::Relaxed) {
			spin_loop();
		}
	}

	#[inline]
	unsafe fn unlock_exclusive(&self) {
		// Clear the least-significant byte of rin
		self.rin.fetch_and(ZERO_MASK, Ordering::Relaxed);

		// Increment wout to indicate this write has released the lock
		// Only one writer should ever be here
		self.wout.fetch_add(1, Ordering::Relaxed);
	}

	#[inline]
	fn try_lock_exclusive(&self) -> bool {
		let wticket = self.win.fetch_add(1, Ordering::Relaxed);
		if wticket != self.wout.load(Ordering::Relaxed) {
			self.wout.fetch_add(1, Ordering::Relaxed);
			return false;
		}
		let w = PRES | (wticket & PHID);
		let rticket = self.rin.fetch_add(w, Ordering::Relaxed);

		if rticket != self.rout.load(Ordering::Relaxed) {
			self.rin.fetch_and(ZERO_MASK, Ordering::Relaxed);
			self.wout.fetch_add(1, Ordering::Relaxed);
			return false;
		}

		true
	}
}

/// A phase-fair reader-writer lock.
pub(crate) type RWLock<T> = RwLock<RWSpinLock, T>;
pub(crate) type RWLockGuard<'a, T> = lock_api::MutexGuard<'a, RWSpinLock, T>;
