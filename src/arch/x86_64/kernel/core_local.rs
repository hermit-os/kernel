use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::asm;
use core::cell::{Cell, RefCell, RefMut};
use core::ptr;
use core::sync::atomic::Ordering;

use x86_64::registers::model_specific::GsBase;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use super::interrupts::{IrqStatistics, IRQ_COUNTERS};
use super::CPU_ONLINE;
use crate::executor::task::AsyncTask;
use crate::scheduler::{CoreId, PerCoreScheduler};

#[repr(C)]
pub(crate) struct CoreLocal {
	this: *const Self,
	/// Sequential ID of this CPU Core.
	core_id: CoreId,
	/// Scheduler for this CPU Core.
	scheduler: Cell<*mut PerCoreScheduler>,
	/// Task State Segment (TSS) allocated for this CPU Core.
	pub tss: Cell<*mut TaskStateSegment>,
	/// start address of the kernel stack
	pub kernel_stack: Cell<u64>,
	/// Interface to the interrupt counters
	irq_statistics: &'static IrqStatistics,
	/// Queue of async tasks
	async_tasks: RefCell<Vec<AsyncTask>>,
}

impl CoreLocal {
	pub fn install() {
		assert_eq!(VirtAddr::zero(), GsBase::read());

		let core_id = CPU_ONLINE.load(Ordering::Relaxed);

		let irq_statistics = &*Box::leak(Box::new(IrqStatistics::new()));
		IRQ_COUNTERS.lock().insert(core_id, irq_statistics);

		let this = Self {
			this: ptr::null_mut(),
			core_id,
			scheduler: Cell::new(ptr::null_mut()),
			tss: Cell::new(ptr::null_mut()),
			kernel_stack: Cell::new(0),
			irq_statistics,
			async_tasks: RefCell::new(Vec::new()),
		};
		let this = Box::leak(Box::new(this));
		this.this = &*this;

		GsBase::write(VirtAddr::from_ptr(this));
	}

	#[inline]
	pub fn get() -> &'static Self {
		debug_assert_ne!(VirtAddr::zero(), GsBase::read());
		unsafe {
			let raw: *const Self;
			asm!("mov {}, gs:0", out(reg) raw, options(nomem, nostack, preserves_flags));
			&*raw
		}
	}
}

pub(crate) fn core_id() -> CoreId {
	if cfg!(target_os = "none") {
		CoreLocal::get().core_id
	} else {
		0
	}
}

pub(crate) fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { &mut *CoreLocal::get().scheduler.get() }
}

pub(crate) fn async_tasks() -> RefMut<'static, Vec<AsyncTask>> {
	CoreLocal::get().async_tasks.borrow_mut()
}

pub(crate) fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	CoreLocal::get().scheduler.set(scheduler);
}

pub(crate) fn increment_irq_counter(irq_no: u8) {
	CoreLocal::get().irq_statistics.inc(irq_no);
}
