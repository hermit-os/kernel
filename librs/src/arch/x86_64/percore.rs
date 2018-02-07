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

use arch::x86_64::processor;
use core::{mem, ptr};
use scheduler::PerCoreScheduler;
use x86::bits64::task::TaskStateSegment;


extern "C" {
	static current_percore_address: usize;
}


#[no_mangle]
pub static mut PERCORE: PerCoreVariables = PerCoreVariables::new(0);


pub struct PerCoreVariables {
	/// APIC ID of this CPU Core.
	core_id: PerCoreVariable<u32>,
	/// Scheduler for this CPU Core.
	scheduler: PerCoreVariable<*mut PerCoreScheduler>,
	/// Task State Segment (TSS) allocated for this CPU Core.
	pub tss: PerCoreVariable<*mut TaskStateSegment>,
	/// Value returned by RDTSC/RDTSCP last time the timer ticks were updated in processor::update_timer_ticks.
	pub last_rdtsc: PerCoreVariable<u64>,
	/// Counted ticks of a timer with the constant frequency specified in processor::TIMER_FREQUENCY.
	pub timer_ticks: PerCoreVariable<usize>,
}

impl PerCoreVariables {
	pub const fn new(core_id: u32) -> Self {
		Self {
			core_id: PerCoreVariable::new(core_id),
			scheduler: PerCoreVariable::new(0 as *mut PerCoreScheduler),
			tss: PerCoreVariable::new(0 as *mut TaskStateSegment),
			last_rdtsc: PerCoreVariable::new(0),
			timer_ticks: PerCoreVariable::new(0),
		}
	}
}


#[repr(C)]
pub struct PerCoreVariable<T> {
	data: T,
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

	#[inline]
	pub unsafe fn get(&self) -> T {
		let value: T;

		match mem::size_of::<T>() {
			4 => asm!("movl %gs:($1), $0" : "=r"(value) : "r"(self.offset()) :: "volatile"),
			8 => asm!("movq %gs:($1), $0" : "=r"(value) : "r"(self.offset()) :: "volatile"),
			_ => panic!("Invalid operand size for get"),
		}

		value
	}

	#[inline]
	pub unsafe fn set(&self, value: T) {
		match mem::size_of::<T>() {
			4 => asm!("movl $0, %gs:($1)" :: "r"(value), "r"(self.offset()) :: "volatile"),
			8 => asm!("movq $0, %gs:($1)" :: "r"(value), "r"(self.offset()) :: "volatile"),
			_ => panic!("Invalid operand size for set"),
		}
	}
}


#[inline]
pub fn core_id() -> u32 {
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
		processor::writegs(address);
	}
}
