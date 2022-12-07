use core::arch::asm;
use core::sync::atomic::{AtomicPtr, Ordering};
use core::{mem, ptr};

use crossbeam_utils::CachePadded;
use x86::msr::*;
use x86_64::structures::tss::TaskStateSegment;

use crate::arch::x86_64::kernel::interrupts::IrqStatistics;
use crate::scheduler::{CoreId, PerCoreScheduler};

pub static mut CORE_LOCAL: CoreLocal = CachePadded::new(CoreLocalInner::new(0));

pub type CoreLocal = CachePadded<CoreLocalInner>;

pub struct CoreLocalInner {
	/// Sequential ID of this CPU Core.
	core_id: CoreLocalVariable<CoreId>,
	/// Scheduler for this CPU Core.
	scheduler: CoreLocalVariable<*mut PerCoreScheduler>,
	/// Task State Segment (TSS) allocated for this CPU Core.
	pub tss: CoreLocalVariable<*mut TaskStateSegment>,
	/// start address of the kernel stack
	pub kernel_stack: CoreLocalVariable<u64>,
	/// Interface to the interrupt counters
	pub irq_statistics: CoreLocalVariable<*mut IrqStatistics>,
}

impl CoreLocalInner {
	pub const fn new(core_id: CoreId) -> Self {
		Self {
			core_id: CoreLocalVariable::new(core_id),
			scheduler: CoreLocalVariable::new(ptr::null_mut() as *mut PerCoreScheduler),
			tss: CoreLocalVariable::new(ptr::null_mut() as *mut TaskStateSegment),
			kernel_stack: CoreLocalVariable::new(0),
			irq_statistics: CoreLocalVariable::new(ptr::null_mut() as *mut IrqStatistics),
		}
	}
}

#[repr(C)]
pub struct CoreLocalVariable<T> {
	data: T,
}

impl<T> CoreLocalVariable<T> {
	pub const fn new(value: T) -> Self {
		Self { data: value }
	}

	#[inline]
	unsafe fn offset(&self) -> usize {
		let base = unsafe { &CORE_LOCAL } as *const _ as usize;
		let field = self as *const _ as usize;
		field - base
	}
}

impl<T> CoreLocalVariable<T> {
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
				*ptr::addr_of_mut!(CORE_LOCAL)
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
				*ptr::addr_of_mut!(CORE_LOCAL)
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
	unsafe { CORE_LOCAL.core_id.get() }
}

#[cfg(not(target_os = "none"))]
pub fn core_id() -> CoreId {
	0
}

#[inline(always)]
pub fn get_kernel_stack() -> u64 {
	unsafe { CORE_LOCAL.kernel_stack.get() }
}

#[inline]
pub fn set_kernel_stack(addr: u64) {
	unsafe { CORE_LOCAL.kernel_stack.set(addr) }
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

#[inline]
pub fn increment_irq_counter(irq_no: usize) {
	unsafe {
		let irq = &mut *CORE_LOCAL.irq_statistics.get();
		irq.inc(irq_no);
	}
}

pub static CURRENT_CORE_LOCAL_ADDRESS: AtomicPtr<CoreLocal> = AtomicPtr::new(ptr::null_mut());

pub fn init() {
	// Store the address to the CoreLocal structure allocated for this core in GS.
	let ptr = {
		let ptr = CURRENT_CORE_LOCAL_ADDRESS.load(Ordering::Relaxed);
		if ptr.is_null() {
			unsafe { ptr::addr_of_mut!(CORE_LOCAL) }
		} else {
			ptr
		}
	};

	unsafe {
		wrmsr(IA32_GS_BASE, ptr as u64);
	}
}
