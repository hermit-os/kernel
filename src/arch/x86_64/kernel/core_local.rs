use alloc::boxed::Box;
use core::arch::asm;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use x86_64::registers::model_specific::GsBase;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use super::interrupts::{IrqStatistics, IRQ_COUNTERS};
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
	pub kernel_stack: u64,
	/// Interface to the interrupt counters
	irq_statistics: *mut IrqStatistics,
}

impl CoreLocal {
	pub fn leak_new(core_id: CoreId) -> &'static mut Self {
		let this = Self {
			this: ptr::null_mut(),
			core_id,
			scheduler: ptr::null_mut(),
			tss: ptr::null_mut(),
			kernel_stack: 0,
			irq_statistics: ptr::null_mut(),
		};
		let mut this = Box::leak(Box::new(this));
		this.this = &*this;

		let irq_statistics = Box::leak(Box::new(IrqStatistics::new()));
		this.irq_statistics = irq_statistics;
		unsafe {
			IRQ_COUNTERS.insert(core_id, irq_statistics);
		}

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

pub static CURRENT_CORE_LOCAL_ADDRESS: AtomicPtr<CoreLocal> = AtomicPtr::new(ptr::null_mut());

pub fn init() {
	// Store the address to the CoreLocal structure allocated for this core in GS.
	let ptr = {
		let ptr = CURRENT_CORE_LOCAL_ADDRESS.load(Ordering::Relaxed);
		if ptr.is_null() {
			CoreLocal::leak_new(0)
		} else {
			ptr
		}
	};

	GsBase::write(VirtAddr::from_ptr(ptr));
}
