// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! The Memory Manager manages heap-allocated memory, but also allocates and
//! deallocates Free List nodes on the heap itself.
//! If the Global MM Lock was just a simple spinlock, the first recursive call
//! from the Memory Manager into another Memory Manager function would lead to
//! a deadlock.
//!
//! Therefore, an MM Lock has been implemented that checks if the call originates
//! from the same CPU core and grants MM access in this case.
//! Doing a check per CPU core is sufficient, because our kernel code runs
//! single-threaded on each CPU core.

use arch::percore::*;
use core::sync::atomic::{AtomicIsize, Ordering};
use synch::spinlock::{SpinlockIrqSave, SpinlockIrqSaveGuard};


/// Indicates that no CPU Core is currently using the Memory Manager.
const MM_NO_CORE: isize = -1;


pub struct MmLock {
	current_mm_core: AtomicIsize,
	spinlock: SpinlockIrqSave<()>,
}

pub struct MmLockGuard<'a> {
	current_mm_core: &'a AtomicIsize,
	spinlock_guard: Option<SpinlockIrqSaveGuard<'a, ()>>,
}

impl MmLock {
	pub const fn new() -> Self {
		Self {
			current_mm_core: AtomicIsize::new(MM_NO_CORE),
			spinlock: SpinlockIrqSave::new(()),
		}
	}

	pub fn lock(&self) -> MmLockGuard {
		let core_id = core_id() as isize;

		// Check if this CPU core is currently using the Memory Manager?
		if self.current_mm_core.load(Ordering::SeqCst) == core_id {
			// It is, so we can grant this access and don't need to clean up a spinlock later.
			MmLockGuard {
				current_mm_core: &self.current_mm_core,
				spinlock_guard: None,
			}
		} else {
			// It is not, so forward this request to the Spinlock.
			let lock = self.spinlock.lock();

			// We successfully acquired the MM lock.
			// Store our CPU core as the current MM user.
			self.current_mm_core.store(core_id, Ordering::SeqCst);

			// Return a MmLockGuard containing the SpinlockGuard (see also below).
			MmLockGuard {
				current_mm_core: &self.current_mm_core,
				spinlock_guard: Some(lock),
			}
		}
	}
}

impl<'a> Drop for MmLockGuard<'a> {
	fn drop(&mut self) {
		// Check if the returned MmLockGuard from the lock contains a spinlock guard.
		if let Some(ref _lock) = self.spinlock_guard {
			// It does. This means that we were the caller that originally acquired a lock for
			// the Memory Manager for this CPU core.
			// We also have to do the cleanup now and remove us as the current user of the MM.
			self.current_mm_core.store(MM_NO_CORE, Ordering::SeqCst);
		}
	}
}
