use core::ptr;

use crate::scheduler::{CoreId, PerCoreScheduler};

#[no_mangle]
pub static mut CORE_LOCAL: CoreLocal = CoreLocal::new(0);

pub struct CoreLocal {
	/// ID of the current Core.
	core_id: CoreLocalVariable<CoreId>,
	/// Scheduler of the current Core.
	scheduler: CoreLocalVariable<*mut PerCoreScheduler>,
}

impl CoreLocal {
	pub const fn new(core_id: CoreId) -> Self {
		Self {
			core_id: CoreLocalVariable::new(core_id),
			scheduler: CoreLocalVariable::new(0 as *mut PerCoreScheduler),
		}
	}
}

#[repr(C)]
pub struct CoreLocalVariable<T> {
	data: T,
}

pub trait CoreLocalVariableMethods<T: Clone> {
	unsafe fn get(&self) -> T;
	unsafe fn set(&mut self, value: T);
}

impl<T> CoreLocalVariable<T> {
	const fn new(value: T) -> Self {
		Self { data: value }
	}
}

// Treat all per-core variables as 64-bit variables by default. This is true for u64, usize, pointers.
// Implement the CoreLocalVariableMethods trait functions using 64-bit memory moves.
// The functions are implemented as default functions, which can be overridden in specialized implementations of the trait.
impl<T> CoreLocalVariableMethods<T> for CoreLocalVariable<T>
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
	unsafe { CORE_LOCAL.core_id.get() }
}

#[inline]
pub fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { &mut *CORE_LOCAL.scheduler.get() }
}

#[inline]
pub fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	unsafe {
		CORE_LOCAL.scheduler.set(scheduler);
	}
}

pub fn init() {
	// TODO: Implement!
}
