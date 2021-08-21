// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::scheduler::{CoreId, PerCoreScheduler};
use core::ptr;

#[no_mangle]
pub static mut PERCORE: PerCoreVariables = PerCoreVariables::new(0);

pub struct PerCoreVariables {
	/// ID of the current Core.
	core_id: PerCoreVariable<CoreId>,
	/// Scheduler of the current Core.
	scheduler: PerCoreVariable<*mut PerCoreScheduler>,
}

impl PerCoreVariables {
	pub const fn new(core_id: CoreId) -> Self {
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
impl<T> PerCoreVariableMethods<T> for PerCoreVariable<T>
where
	T: Clone,
{
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
pub fn core_id() -> CoreId {
	unsafe { PERCORE.core_id.get() }
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

pub fn init() {
	// TODO: Implement!
}
