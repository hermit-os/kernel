use core::ops::{Deref, DerefMut};

use crate::core_local::core_scheduler;
#[cfg(feature = "net")]
use crate::executor::network::NETWORK_WAKER;
use crate::set_oneshot_timer;

/// A possible timer interrupt source (i.e. reason the timer interrupt was set
/// up).
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
	Network,
	Scheduler,
}

/// A slot in the timer list. Each source is represented once. This is so that
/// we can have multiple timers at the same time with only one hardware timer.
#[derive(Debug)]
pub struct Slot {
	/// Timer source.
	source: Source,
	/// Point in time at which to wake up (in microsecond precision).
	/// A value of [`u64::MAX`] means the timer is not set.
	wakeup_time: u64,
}

// List of timers with one entry for every possible source.
#[derive(Debug)]
pub struct TimerList([Slot; 2]);

impl TimerList {
	pub fn new() -> Self {
		Self([
			Slot {
				source: Source::Network,
				wakeup_time: u64::MAX,
			},
			Slot {
				source: Source::Scheduler,
				wakeup_time: u64::MAX,
			},
		])
	}
}

impl Deref for TimerList {
	type Target = [Slot; 2];

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for TimerList {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

/// Create a new timer, overriding any previous timer for the source.
#[cfg(feature = "net")]
#[inline]
pub fn create_timer(source: Source, wakeup_micros: u64) {
	create_timer_abs(
		source,
		crate::arch::processor::get_timer_ticks() + wakeup_micros,
	);
}

/// Crete a new timer, but with an absolute wakeup time.
pub fn create_timer_abs(source: Source, wakeup_time: u64) {
	let timers = &mut core_scheduler().timers;

	// SAFETY: Our timer list has an entry for every possible source
	let previous_entry = timers
		.iter_mut()
		.find(|slot| slot.source == source)
		.unwrap();

	// Overwrite the wakeup time
	previous_entry.wakeup_time = previous_entry.wakeup_time.min(wakeup_time);

	// If this timer is the one closest in the future, set the real timer to it
	// SAFETY: There's more than 1 slot
	if timers.iter().map(|slot| slot.wakeup_time).min().unwrap() == wakeup_time {
		set_oneshot_timer(Some(wakeup_time));
	}
}

/// Clears the timer slot for the currently active timer and sets the next timer or disables it if no timer is pending.
pub fn clear_active_and_set_next() {
	let timers = &mut core_scheduler().timers;

	// SAFETY: There's more than 1 slot
	let lowest_timer = timers
		.iter_mut()
		.min_by(|a, b| a.wakeup_time.cmp(&b.wakeup_time))
		.unwrap();

	assert!(lowest_timer.wakeup_time != u64::MAX);

	// TODO: Do we really want to do this here?
	match lowest_timer.source {
		#[cfg(feature = "net")]
		Source::Network => NETWORK_WAKER.lock().wake(),
		_ => {} // no-op, we always poll after a timer interrupt
	}

	trace!("Cleared active timer {lowest_timer:?}");

	lowest_timer.wakeup_time = u64::MAX;

	// We may receive a timer interrupt earlier than expected
	// This appears to only be the case in QEMU, it seems like timer ticks
	// do not advance linearly there?
	// Either way, this means that QEMU *thinks* the time has passed, so it
	// probably has and knows better than we do.
	// We can cheat a bit and adjust all timers slightly based on this
	if lowest_timer.wakeup_time > crate::arch::processor::get_timer_ticks() {
		let offset = lowest_timer.wakeup_time - crate::arch::processor::get_timer_ticks();

		for timer in timers.iter_mut() {
			if timer.wakeup_time != u64::MAX {
				timer.wakeup_time -= offset;
			}
		}
	}

	// SAFETY: There's more than 1 slot
	let new_lowest_timer = timers.iter().map(|slot| slot.wakeup_time).min().unwrap();

	if new_lowest_timer == u64::MAX {
		set_oneshot_timer(None);
	} else {
		set_oneshot_timer(Some(new_lowest_timer));
	}
}
