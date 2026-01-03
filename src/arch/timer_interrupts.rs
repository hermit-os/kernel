use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "net")]
use crate::executor::network::NETWORK_WAKER;
use crate::set_oneshot_timer;

/// A possible timer interrupt source (i.e. reason the timer interrupt was set
/// up).
#[derive(PartialEq, Eq)]
pub enum Source {
	Network,
	Scheduler,
}

/// A slot in the timer list. Each source is represented once. This is so that
/// we can have multiple timers at the same time with only one hardware timer.
struct Slot {
	/// Timer source.
	source: Source,
	/// Point in time at which to wake up (in microsecond precision).
	/// A value of [`u64::MAX`] means the timer is not set.
	wakeup_time: AtomicU64,
}

/// The actual timer list with one entry for each source.
static TIMERS: [Slot; 2] = [
	Slot {
		source: Source::Network,
		wakeup_time: AtomicU64::new(u64::MAX),
	},
	Slot {
		source: Source::Scheduler,
		wakeup_time: AtomicU64::new(u64::MAX),
	},
];

/// Create a new timer, overriding any previous timer for the source.
#[cfg(feature = "net")]
pub fn create_timer(source: Source, wakeup_micros: u64) {
	trace!("Setting relative timer interrupt for {wakeup_micros}us");

	create_timer_abs(
		source,
		crate::arch::processor::get_timer_ticks() + wakeup_micros,
	);
}

/// Crete a new timer, but with an absolute wakeup time.
pub fn create_timer_abs(source: Source, wakeup_time: u64) {
	trace!(
		"Setting an absolute timer interrupt for {}us",
		wakeup_time - crate::arch::processor::get_timer_ticks()
	);

	{
		// SAFETY: Our timer list has an entry for every possible source
		let previous_entry = TIMERS.iter().find(|slot| slot.source == source).unwrap();

		// Overwrite the wakeup time
		previous_entry
			.wakeup_time
			.store(wakeup_time, Ordering::Relaxed);
	}

	// If this timer is the one closest in the future, set the real timer to it
	// SAFETY: There's more than 1 slot
	if TIMERS
		.iter()
		.map(|slot| slot.wakeup_time.load(Ordering::Relaxed))
		.min_by(|a, b| a.cmp(b))
		.unwrap()
		== wakeup_time
	{
		trace!("Setting the oneshot timer now");

		set_oneshot_timer(Some(wakeup_time));
	}
}

/// Sets the next timer or disables it if no timer is pending.
pub fn set_next_timer() {
	// SAFETY: There's more than 1 slot
	let lowest_timer = TIMERS
		.iter()
		.map(|slot| slot.wakeup_time.load(Ordering::Relaxed))
		.min_by(|a, b| a.cmp(b))
		.unwrap();

	if lowest_timer == u64::MAX {
		set_oneshot_timer(None);
	} else {
		set_oneshot_timer(Some(lowest_timer));
	}
}

/// Clears the timer slot for the currently active timer.
pub fn clear_active() {
	// SAFETY: There's more than 1 slot
	let lowest_timer = TIMERS
		.iter()
		.min_by(|a, b| {
			a.wakeup_time
				.load(Ordering::Relaxed)
				.cmp(&b.wakeup_time.load(Ordering::Relaxed))
		})
		.unwrap();

	// TODO: Do we really want to do this here?
	match lowest_timer.source {
		#[cfg(feature = "net")]
		Source::Network => NETWORK_WAKER.lock().wake(),
		_ => {} // no-op, we always poll after a timer interrupt
	}

	trace!("Cleared active timer");

	lowest_timer.wakeup_time.store(u64::MAX, Ordering::Relaxed);
}
