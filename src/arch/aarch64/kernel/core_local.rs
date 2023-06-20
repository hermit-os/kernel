use alloc::boxed::Box;
use core::arch::asm;
use core::cell::Cell;
use core::ptr;
use core::sync::atomic::Ordering;

use super::interrupts::{IrqStatistics, IRQ_COUNTERS};
use super::CPU_ONLINE;
use crate::scheduler::{CoreId, PerCoreScheduler};

pub struct CoreLocal {
	this: *const Self,
	/// ID of the current Core.
	core_id: CoreId,
	/// Scheduler of the current Core.
	scheduler: Cell<*mut PerCoreScheduler>,
	/// Interface to the interrupt counters
	irq_statistics: &'static IrqStatistics,
}

impl CoreLocal {
	pub fn install() {
		let core_id = CPU_ONLINE.load(Ordering::Relaxed);

		let irq_statistics = &*Box::leak(Box::new(IrqStatistics::new()));
		IRQ_COUNTERS.lock().insert(core_id, irq_statistics);

		let this = Self {
			this: ptr::null_mut(),
			core_id,
			scheduler: Cell::new(ptr::null_mut()),
			irq_statistics,
		};
		let mut this = Box::leak(Box::new(this));
		this.this = &*this;

		unsafe {
			asm!("msr tpidr_el1, {}", in(reg) this, options(nomem, nostack, preserves_flags));
		}
	}

	#[inline]
	pub fn get() -> &'static Self {
		unsafe {
			let raw: *const Self;
			asm!("mrs {}, tpidr_el1", out(reg) raw, options(nomem, nostack, preserves_flags));
			&*raw
		}
	}
}

#[inline]
pub fn core_id() -> CoreId {
	if cfg!(target_os = "none") {
		CoreLocal::get().core_id
	} else {
		0
	}
}

#[inline]
pub fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { &mut *CoreLocal::get().scheduler.get() }
}

pub fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	CoreLocal::get().scheduler.set(scheduler);
}

pub fn increment_irq_counter(irq_no: u8) {
	CoreLocal::get().irq_statistics.inc(irq_no);
}
