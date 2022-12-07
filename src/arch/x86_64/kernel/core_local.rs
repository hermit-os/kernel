use alloc::boxed::Box;
use core::arch::asm;
use core::ptr;
use core::sync::atomic::Ordering;

use x86_64::registers::model_specific::GsBase;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use super::interrupts::{IrqStatistics, IRQ_COUNTERS};
use super::CPU_ONLINE;
use crate::scheduler::{CoreId, PerCoreScheduler};

pub struct CoreLocal {
	this: *const Self,
	/// Sequential ID of this CPU Core.
	core_id: CoreId,
	/// Scheduler for this CPU Core.
	scheduler: *mut PerCoreScheduler,
	/// Task State Segment (TSS) allocated for this CPU Core.
	pub tss: *mut TaskStateSegment,
	/// start address of the kernel stack
	kernel_stack: u64,
	/// Interface to the interrupt counters
	irq_statistics: *mut IrqStatistics,
}

impl CoreLocal {
	pub fn leak_new(core_id: CoreId) -> &'static mut Self {
		let irq_statistics = Box::leak(Box::new(IrqStatistics::new()));
		unsafe {
			// FIXME: This is a very illegal reborrow
			IRQ_COUNTERS.insert(core_id, &*(irq_statistics as *const _));
		}

		let this = Self {
			this: ptr::null_mut(),
			core_id,
			scheduler: ptr::null_mut(),
			tss: ptr::null_mut(),
			kernel_stack: 0,
			irq_statistics,
		};
		let mut this = Box::leak(Box::new(this));
		this.this = &*this;

		this
	}

	pub fn get_raw() -> *mut Self {
		debug_assert_ne!(VirtAddr::zero(), GsBase::read());
		let raw;
		unsafe {
			asm!("mov {}, gs:0", out(reg) raw, options(nomem, nostack, preserves_flags));
		}
		raw
	}
}

#[cfg(target_os = "none")]
#[inline]
pub fn core_id() -> CoreId {
	unsafe { (*CoreLocal::get_raw()).core_id }
}

#[cfg(not(target_os = "none"))]
pub fn core_id() -> CoreId {
	0
}

#[inline(always)]
pub fn get_kernel_stack() -> u64 {
	unsafe { (*CoreLocal::get_raw()).kernel_stack }
}

#[inline]
pub fn set_kernel_stack(addr: u64) {
	unsafe {
		(*CoreLocal::get_raw()).kernel_stack = addr;
	}
}

#[inline]
pub fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { &mut *(*CoreLocal::get_raw()).scheduler }
}

#[inline]
pub fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	unsafe {
		(*CoreLocal::get_raw()).scheduler = scheduler;
	}
}

#[inline]
pub fn increment_irq_counter(irq_no: usize) {
	unsafe {
		let irq = &mut *(*CoreLocal::get_raw()).irq_statistics;
		irq.inc(irq_no);
	}
}

pub fn init() {
	debug_assert_eq!(VirtAddr::zero(), GsBase::read());

	let core_id = CPU_ONLINE.load(Ordering::Relaxed);
	let core_local = CoreLocal::leak_new(core_id);

	GsBase::write(VirtAddr::from_ptr(core_local));
}
