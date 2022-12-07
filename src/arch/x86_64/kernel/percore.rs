use core::arch::asm;
use core::sync::atomic::{AtomicPtr, Ordering};
use core::{mem, ptr};

use crossbeam_utils::CachePadded;
use x86::msr::*;
use x86_64::structures::tss::TaskStateSegment;

use crate::arch::x86_64::kernel::interrupts::IrqStatistics;
use crate::scheduler::{CoreId, PerCoreScheduler};

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

impl<T> PerCoreVariable<T> {
	pub const fn new(value: T) -> Self {
		Self { data: value }
	}

	#[inline]
	unsafe fn offset(&self) -> usize {
		let base = unsafe { &PERCORE } as *const _ as usize;
		let field = self as *const _ as usize;
		field - base
	}
}

impl<T> PerCoreVariable<T> {
	#[inline]
	pub unsafe fn get(&self) -> T
	where
		T: Copy,
	{
		if cfg!(feature = "smp") {
			match mem::size_of::<T>() {
				8 => unsafe {
					let value: u64;
					asm!(
						"mov {}, gs:[{}]",
						lateout(reg) value,
						in(reg) self.offset(),
						options(pure, readonly, nostack, preserves_flags),
					);
					mem::transmute_copy(&value)
				},
				4 => unsafe {
					let value: u32;
					asm!(
						"mov {:e}, gs:[{}]",
						lateout(reg) value,
						in(reg) self.offset(),
						options(pure, readonly, nostack, preserves_flags),
					);
					mem::transmute_copy(&value)
				},
				_ => unreachable!(),
			}
		} else {
			unsafe {
				*ptr::addr_of_mut!(PERCORE)
					.cast::<u8>()
					.add(self.offset())
					.cast()
			}
		}
	}

	#[inline]
	pub unsafe fn set(&self, value: T) {
		if cfg!(feature = "smp") {
			match mem::size_of::<T>() {
				8 => unsafe {
					let value = mem::transmute_copy::<_, u64>(&value);
					asm!(
						"mov gs:[{}], {}",
						in(reg) self.offset(),
						in(reg) value,
						options(nostack, preserves_flags),
					);
				},
				4 => unsafe {
					let value = mem::transmute_copy::<_, u32>(&value);
					asm!(
						"mov gs:[{}], {:e}",
						in(reg) self.offset(),
						in(reg) value,
						options(nostack, preserves_flags),
					);
				},
				_ => unreachable!(),
			}
		} else {
			unsafe {
				*ptr::addr_of_mut!(PERCORE)
					.cast::<u8>()
					.add(self.offset())
					.cast() = value;
			}
		}
	}
}

#[cfg(target_os = "none")]
#[inline]
pub fn core_id() -> CoreId {
	unsafe { PERCORE.core_id.get() }
}

#[cfg(not(target_os = "none"))]
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

pub static CURRENT_PERCORE_ADDRESS: AtomicPtr<PerCoreVariables> = AtomicPtr::new(ptr::null_mut());

pub fn init() {
	// Store the address to the PerCoreVariables structure allocated for this core in GS.
	let ptr = {
		let ptr = CURRENT_PERCORE_ADDRESS.load(Ordering::Relaxed);
		if ptr.is_null() {
			unsafe { ptr::addr_of_mut!(PERCORE) }
		} else {
			ptr
		}
	};

	unsafe {
		wrmsr(IA32_GS_BASE, ptr as u64);
	}
}
