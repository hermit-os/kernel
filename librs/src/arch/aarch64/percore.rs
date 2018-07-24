// Copyright (c) 2018 Colin Finck, RWTH Aachen University
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

#[no_mangle]
pub static mut PERCORE: PerCoreVariables = PerCoreVariables::new(0);


pub struct PerCoreVariables {
	/// APIC ID of this CPU Core.
	core_id: PerCoreVariable<usize>,
	/// Scheduler for this CPU Core.
	scheduler: PerCoreVariable<*mut PerCoreScheduler>,
}

impl PerCoreVariables {
	pub const fn new(core_id: usize) -> Self {
		Self {
			core_id: PerCoreVariable::new(core_id),
			scheduler: PerCoreVariable::new(0 as *mut PerCoreScheduler),
		}
	}
}


#[repr(C)]
pub struct PerCoreVariable<T> {
	data: T,
}

pub trait PerCoreVariableMethods<T: Clone> {
	unsafe fn get(&self) -> T;
	unsafe fn set(&mut self, value: T);
}

impl<T> PerCoreVariable<T> {
	const fn new(value: T) -> Self {
		Self { data: value }
	}
}

// Treat all per-core variables as 64-bit variables by default. This is true for u64, usize, pointers.
// Implement the PerCoreVariableMethods trait functions using 64-bit memory moves.
// The functions are implemented as default functions, which can be overriden in specialized implementations of the trait.
impl<T> PerCoreVariableMethods<T> for PerCoreVariable<T> where T: Clone {
	#[inline]
	default unsafe fn get(&self) -> T {
		self.data.clone()
	}

	#[inline]
	default unsafe fn set(&mut self, value: T) {
		self.data = value;
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
	// TODO: Implement!
}
