// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use core::ptr;
use scheduler::PerCoreScheduler;
use x86::bits64::task::TaskStateSegment;
use x86::msr::*;

extern "C" {
	static current_percore_address: usize;
}


#[no_mangle]
pub static mut PERCORE: PerCoreVariables = PerCoreVariables::new(0);


pub struct PerCoreVariables {
	/// Sequential ID of this CPU Core.
	core_id: PerCoreVariable<usize>,
	/// Scheduler for this CPU Core.
	scheduler: PerCoreVariable<*mut PerCoreScheduler>,
	/// Task State Segment (TSS) allocated for this CPU Core.
	pub tss: PerCoreVariable<*mut TaskStateSegment>,
}

impl PerCoreVariables {
	pub const fn new(core_id: usize) -> Self {
		Self {
			core_id: PerCoreVariable::new(core_id),
			scheduler: PerCoreVariable::new(0 as *mut PerCoreScheduler),
			tss: PerCoreVariable::new(0 as *mut TaskStateSegment),
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
	const fn new(value: T) -> Self {
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
// The functions are implemented as default functions, which can be overriden in specialized implementations of the trait.
impl<T> PerCoreVariableMethods<T> for PerCoreVariable<T> {
	#[inline]
	default unsafe fn get(&self) -> T {
		let value: T;
		asm!("movq %gs:($1), $0" : "=r"(value) : "r"(self.offset()) :: "volatile");
		value
	}

	#[inline]
	default unsafe fn set(&self, value: T) {
		asm!("movq $0, %gs:($1)" :: "r"(value), "r"(self.offset()) :: "volatile");
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
		asm!("movl %gs:($1), $0" : "=r"(value) : "r"(self.offset()) :: "volatile");
		value
	}

	#[inline]
	unsafe fn set(&self, value: T) {
		asm!("movl $0, %gs:($1)" :: "r"(value), "r"(self.offset()) :: "volatile");
	}
}


#[inline]
pub fn core_id() -> usize {
	unsafe { PERCORE.core_id.get() }
}

#[inline]
pub fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { &mut *PERCORE.scheduler.get() }
}

#[inline]
pub fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	unsafe { PERCORE.scheduler.set(scheduler); }
}

pub fn init() {
	unsafe {
		// Store the address to the PerCoreVariables structure allocated for this core in GS.
		let address = ptr::read_volatile(&current_percore_address);
		wrmsr(IA32_GS_BASE, address as u64);
	}
}
