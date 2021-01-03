// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::x86_64::kernel::irq::IrqStatistics;
use crate::arch::x86_64::kernel::BOOT_INFO;
use crate::scheduler::{CoreId, PerCoreScheduler};
use crate::x86::bits64::task::TaskStateSegment;
use crate::x86::msr::*;
use core::ptr;
use crossbeam_utils::CachePadded;

pub static mut PERCORE: PerCoreVariables = CachePadded::new(PerCoreInnerVariables::new(0));

pub type PerCoreVariables = CachePadded<PerCoreInnerVariables>;

pub struct PerCoreInnerVariables {
	/// Sequential ID of this CPU Core.
	core_id: PerCoreVariable<CoreId>,
	/// Scheduler for this CPU Core.
	scheduler: PerCoreVariable<*mut PerCoreScheduler>,
	/// Task State Segment (TSS) allocated for this CPU Core.
	pub tss: PerCoreVariable<*mut TaskStateSegment>,
	/// start address of the kernel stack
	pub kernel_stack: PerCoreVariable<u64>,
	/// Interface to the interrupt counters
	pub irq_statistics: PerCoreVariable<*mut IrqStatistics>,
}

impl PerCoreInnerVariables {
	pub const fn new(core_id: CoreId) -> Self {
		Self {
			core_id: PerCoreVariable::new(core_id),
			scheduler: PerCoreVariable::new(ptr::null_mut() as *mut PerCoreScheduler),
			tss: PerCoreVariable::new(ptr::null_mut() as *mut TaskStateSegment),
			kernel_stack: PerCoreVariable::new(0),
			irq_statistics: PerCoreVariable::new(ptr::null_mut() as *mut IrqStatistics),
		}
	}
}

#[repr(C)]
pub struct PerCoreVariable<T> {
	data: T,
}

pub trait PerCoreVariableMethods<T> {
	unsafe fn get(&self) -> T;
	unsafe fn set(&self, value: T);
}

impl<T> PerCoreVariable<T> {
	pub const fn new(value: T) -> Self {
		Self { data: value }
	}

	#[inline]
	unsafe fn offset(&self) -> usize {
		let base = &PERCORE as *const _ as usize;
		let field = self as *const _ as usize;
		field - base
	}
}

// Treat all per-core variables as 64-bit variables by default. This is true for u64, usize, pointers.
// Implement the PerCoreVariableMethods trait functions using 64-bit memory moves.
// The functions are implemented as default functions, which can be overridden in specialized implementations of the trait.
impl<T> PerCoreVariableMethods<T> for PerCoreVariable<T> {
	#[inline]
	default unsafe fn get(&self) -> T {
		let value: T;
		llvm_asm!("movq %gs:($1), $0" : "=r"(value) : "r"(self.offset()) :: "volatile");
		value
	}

	#[inline]
	default unsafe fn set(&self, value: T) {
		llvm_asm!("movq $0, %gs:($1)" :: "r"(value), "r"(self.offset()) :: "volatile");
	}
}

// Define and implement a trait to mark all 32-bit variables used inside PerCoreVariables.
pub trait Is32BitVariable {}
impl Is32BitVariable for u32 {}

// For all types implementing the Is32BitVariable trait above, implement the PerCoreVariableMethods
// trait functions using 32-bit memory moves.
impl<T: Is32BitVariable> PerCoreVariableMethods<T> for PerCoreVariable<T> {
	#[inline]
	unsafe fn get(&self) -> T {
		let value: T;
		llvm_asm!("movl %gs:($1), $0" : "=r"(value) : "r"(self.offset()) :: "volatile");
		value
	}

	#[inline]
	unsafe fn set(&self, value: T) {
		llvm_asm!("movl $0, %gs:($1)" :: "r"(value), "r"(self.offset()) :: "volatile");
	}
}

#[cfg(target_os = "hermit")]
#[inline]
pub fn core_id() -> CoreId {
	unsafe { PERCORE.core_id.get() }
}

#[cfg(not(target_os = "hermit"))]
pub fn core_id() -> CoreId {
	0
}

#[inline(always)]
pub fn get_kernel_stack() -> u64 {
	unsafe { PERCORE.kernel_stack.get() }
}

#[inline]
pub fn set_kernel_stack(addr: u64) {
	unsafe { PERCORE.kernel_stack.set(addr) }
}

#[inline]
pub fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { &mut *PERCORE.scheduler.get() }
}

#[inline]
pub fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	unsafe {
		PERCORE.scheduler.set(scheduler);
	}
}

#[inline]
pub fn increment_irq_counter(irq_no: usize) {
	unsafe {
		let irq = &mut *PERCORE.irq_statistics.get();
		irq.inc(irq_no);
	}
}

pub fn init() {
	unsafe {
		// Store the address to the PerCoreVariables structure allocated for this core in GS.
		let address = core::ptr::read_volatile(&(*BOOT_INFO).current_percore_address);
		if address == 0 {
			wrmsr(IA32_GS_BASE, &PERCORE as *const _ as u64);
		} else {
			wrmsr(IA32_GS_BASE, address as u64);
		}
	}
}
