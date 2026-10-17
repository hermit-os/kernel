//! Per-core log of kernel events, printed on shutdown.

use alloc::boxed::Box;
use alloc::string::String;
use core::fmt::Write;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use hermit_sync::OnceCell;

use crate::arch::kernel::core_local::core_id;
use crate::arch::kernel::processor::{get_frequency, get_timestamp};
use crate::scheduler::CoreId;

const MAX_CORES: usize = 64;

/// Number of events the log of a core keeps.
const CAPACITY: usize = 1 << 16;

const EVENTS_PER_LINE: usize = 16;

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum Event {
	/// The timer was armed with a deadline, given as a timestamp of the processor.
	TimerArm { deadline: u64 },
	/// Entry of the timer handler.
	TimerInterrupt,
	/// An IPI was sent to a core.
	IpiSend { target: CoreId },
	/// Entry of the IPI handler.
	IpiReceive,
}

impl Event {
	/// Name of every kind of event and whether it has an argument.
	const KINDS: [(&str, bool); 4] = [
		("timer_arm", true),
		("timer_irq", false),
		("ipi_send", true),
		("ipi_recv", false),
	];

	fn encode(self) -> (usize, u64) {
		match self {
			Self::TimerArm { deadline } => (0, deadline),
			Self::TimerInterrupt => (1, 0),
			Self::IpiSend { target } => (2, u64::from(target)),
			Self::IpiReceive => (3, 0),
		}
	}
}

struct Slot {
	time: AtomicU64,
	kind: AtomicUsize,
	arg: AtomicU64,
}

struct CoreLog {
	/// Number of events that were logged since boot.
	recorded: AtomicUsize,
	/// Ring buffer. Once it is full, the oldest events are overwritten.
	slots: Box<[Slot]>,
}

static LOGS: [OnceCell<CoreLog>; MAX_CORES] = [const { OnceCell::new() }; MAX_CORES];

fn current_log() -> Option<&'static CoreLog> {
	LOGS.get(core_id() as usize)?.get()
}

pub fn init() {
	let Some(log) = LOGS.get(core_id() as usize) else {
		return;
	};
	log.get_or_init(|| CoreLog {
		recorded: AtomicUsize::new(0),
		slots: (0..CAPACITY)
			.map(|_| Slot {
				time: AtomicU64::new(0),
				kind: AtomicUsize::new(0),
				arg: AtomicU64::new(0),
			})
			.collect(),
	});
}

#[allow(dead_code)]
#[inline]
pub fn record(event: Event) {
	record_at(get_timestamp(), event);
}

#[allow(dead_code)]
pub fn record_at(time: u64, event: Event) {
	let Some(log) = current_log() else {
		return;
	};
	let (kind, arg) = event.encode();

	let index = log.recorded.fetch_add(1, Ordering::Relaxed);
	let slot = &log.slots[index % CAPACITY];
	slot.time.store(time, Ordering::Relaxed);
	slot.kind.store(kind, Ordering::Relaxed);
	slot.arg.store(arg, Ordering::Relaxed);
}

/// Prints the log of every core: a header `event_log core=...`, followed by its
/// events in lines of the form `event_log_events core:time:event[:arg] ...`.
///
/// The events of a core are printed in the order they were logged. An event that
/// was logged with [`record_at`] may follow events with a later time.
pub fn print() {
	// Each line is built first, so it is written to the console at once.
	let mut line = String::new();

	for (core, log) in LOGS.iter().enumerate() {
		let Some(log) = log.get() else {
			continue;
		};
		let recorded = log.recorded.load(Ordering::Relaxed);
		let dropped = recorded.saturating_sub(CAPACITY);

		println!(
			"event_log core={core} frequency_mhz={} events={} dropped={dropped}",
			get_frequency(),
			recorded - dropped
		);

		for first in (dropped..recorded).step_by(EVENTS_PER_LINE) {
			line.clear();
			line.push_str("event_log_events");
			for index in first..recorded.min(first + EVENTS_PER_LINE) {
				let slot = &log.slots[index % CAPACITY];
				let (name, has_arg) = Event::KINDS[slot.kind.load(Ordering::Relaxed)];
				write!(line, " {core}:{}:{name}", slot.time.load(Ordering::Relaxed)).unwrap();
				if has_arg {
					write!(line, ":{}", slot.arg.load(Ordering::Relaxed)).unwrap();
				}
			}
			println!("{line}");
		}
	}
}
