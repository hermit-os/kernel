use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

use hermit_sync::RwSpinLock;

use crate::core_id;
use crate::scheduler::CoreId;

static CORE_HLT_STATE: RwSpinLock<Vec<SleepState>> = RwSpinLock::new(Vec::new());

struct SleepState(AtomicU8);

/// # Race condition prevention
///
/// Two methods matter here: [crate::arch::interrupts::enable_and_wait] and [crate::arch::wakeup_core].
/// `enable_and_wait` checks the status, ensuring it is set to 0+setting it to 1, then sleeps.
/// `wakeup_core` checks the status, ensuring it is set to 1+setting it to 0, then issues interrupt.
///
/// A race would happen if `go_to_sleep` returns true, and `wake_up` returns false.
/// The result of these two methods must therefore be atomically determined in a single operation.
/// No ordering should exist in which that condition can happen.
/// Any other variant is okay:
/// - if `go_to_sleep` is false and `wake_up` is true, we have a un-necessary core wakeup (which is okay)
/// - if both are false, the core is not entering sleep and is therefore not woken up
/// - if both are true, the core sleeps and is woken up
impl SleepState {
	/// The core is currently busy and should not be interrupted for new tasks
	const STATUS_ACTIVE: u8 = 0;

	/// The core is currently halted and can be interrupted for new tasks
	const STATUS_IDLE: u8 = 1;

	/// Another core tried to wake up this core but it was already active.
	const STATUS_DONT_SLEEP: u8 = 2;

	fn new() -> Self {
		Self(AtomicU8::new(SleepState::STATUS_ACTIVE))
	}

	#[inline]
	fn set_active(&self) {
		self.0.store(SleepState::STATUS_ACTIVE, Ordering::SeqCst);
	}

	/// Indicates that this core will go to HLT.
	/// This must be called *before* entering a HLT loop, and *with interrupts disabled*.
	/// Returns a boolean indicating if the core should enter the HLT loop or not
	fn go_to_sleep(&self) -> bool {
		if self
			.0
			.compare_exchange(
				SleepState::STATUS_ACTIVE,
				SleepState::STATUS_IDLE,
				Ordering::SeqCst,
				Ordering::Relaxed,
			)
			.is_err()
		{
			// There is a possible race condition here, because the two atomic operations can be
			// interleaved, but this has no effect on the final result: we're not going to sleep
			// anyway.
			// If the state was already set to ACTIVE, this has no effect.
			// Normally, nothing can set the state to IDLE except this method.
			// So the race should have no effect.
			// IN ANY CASE: as per top-level comments, returning false is the safe default here.
			self.set_active();
			false
		} else {
			// We have correctly read status ACTIVE and set STATUS_IDLE. We go to sleep. This is
			// safe because:
			// - Another `wake_up` could be processing, either BEFORE or AFTER the atomic `swap`.
			//    * BEFORE:
			//      We have set the state to STATUS_IDLE, so `swap` will read that value and
			//      wake_up will return true ==> SAFE.
			//    * AFTER: **We cannot be here!** Indeed, `swap` has set the status to
			//		`STATUS_DONT_SLEEP`, so `compare_exchange` is in the `Err` case.
			true
		}
	}

	/// Request to wake up this core.
	/// Returns a boolean indicating if an interrupt should be sent, or if the core is already active
	fn wake_up(&self) -> bool {
		// Ask the core not to sleep.
		// This makes sure that if the two atomic operations become interleaved, the core will
		// not go to sleep with us assuming it was running.
		let previous_state = self.0.swap(SleepState::STATUS_DONT_SLEEP, Ordering::SeqCst);

		// If the core was idle, we can actually wake it up
		if previous_state == SleepState::STATUS_IDLE {
			// Again, this operation is not necessarily ordered.
			// - Another `go_to_sleep` could be processing, either BEFORE or AFTER the atomic op.
			//    * BEFORE:
			//      We have set the state to STATUS_DONT_SLEEP, so the atomic op will read that value
			//      and go_to_sleep will return false. We will send an interrupt which could have
			//      been avoided, but that's okay.
			//    * AFTER:
			//      The value read at the atomic OP does not matter here, in any case we WILL send
			//      the interrupt and wake the core up.
			// IN ANY CASE: as per top-level comments, returning true is the safe default here.
			self.set_active();
			true
		} else {
			// We don't wake up the core. This is safe, because:
			// - Another `go_to_sleep` could be processing, either BEFORE or AFTER the atomic op.
			//    * BEFORE:
			//      We have set the state to STATUS_DONT_SLEEP, so the atomic op will read that value
			//      and go_to_sleep will return false ==> SAFE.
			//    * AFTER: **We cannot be here!** Indeed, the atomic operation in `go_to_sleep`
			//      also sets the state to STATUS_IDLE, so `previous_state` MUST BE STATUS_IDLE.
			false
		}
	}
}

pub(super) fn install_for_core(core_id: CoreId) {
	CORE_HLT_STATE
		.write()
		.insert(core_id.try_into().unwrap(), SleepState::new());
}

#[inline]
pub fn core_sleep() -> bool {
	CORE_HLT_STATE.read()[usize::try_from(core_id()).unwrap()].go_to_sleep()
}

#[inline]
pub fn core_wake_up(core_id: CoreId) -> bool {
	CORE_HLT_STATE.read()[usize::try_from(core_id).unwrap()].wake_up()
}
