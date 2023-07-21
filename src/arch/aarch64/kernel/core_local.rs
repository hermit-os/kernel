use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::asm;
use core::cell::{Cell, RefCell};
use core::ptr;
use core::sync::atomic::Ordering;

use super::interrupts::{IrqStatistics, IRQ_COUNTERS};
use super::CPU_ONLINE;
use crate::executor::task::AsyncTask;
use crate::scheduler::{CoreId, PerCoreScheduler};

pub(crate) struct CoreLocal {
	this: *const Self,
	/// ID of the current Core.
	core_id: CoreId,
	/// Scheduler of the current Core.
	scheduler: Cell<*mut PerCoreScheduler>,
	/// Interface to the interrupt counters
	irq_statistics: &'static IrqStatistics,
	/// Queue of async tasks
	async_tasks: RefCell<Vec<AsyncTask>>,
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
			async_tasks: RefCell::new(Vec::new()),
		};
		let this = Box::leak(Box::new(this));
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

	#[inline]
	pub fn get_mut() -> &'static mut Self {
		unsafe {
			let raw: *mut Self;
			asm!("mrs {}, tpidr_el1", out(reg) raw, options(nomem, nostack, preserves_flags));
			&mut *raw
		}
	}
}

#[inline]
pub(crate) fn core_id() -> CoreId {
	if cfg!(target_os = "none") {
		CoreLocal::get().core_id
	} else {
		0
	}
}

#[inline]
pub(crate) fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { &mut *CoreLocal::get().scheduler.get() }
}

pub(crate) fn async_tasks() -> &'static mut Vec<AsyncTask> {
	CoreLocal::get_mut().async_tasks.get_mut()
}

pub(crate) fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	CoreLocal::get().scheduler.set(scheduler);
}

pub(crate) fn increment_irq_counter(irq_no: u8) {
	CoreLocal::get().irq_statistics.inc(irq_no);
}
