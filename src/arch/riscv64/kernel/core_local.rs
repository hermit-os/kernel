use alloc::boxed::Box;
use core::arch::asm;
use core::cell::Cell;
use core::ptr;
use core::sync::atomic::Ordering;

use async_executor::StaticLocalExecutor;
#[cfg(feature = "smp")]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::{RawRwSpinLock, RawSpinMutex};

use crate::arch::kernel::CPU_ONLINE;
#[cfg(not(feature = "riscv-plic"))]
use crate::arch::kernel::interrupts::MsiController;
#[cfg(feature = "smp")]
use crate::scheduler::SchedulerInput;
use crate::scheduler::{CoreId, PerCoreScheduler};

pub struct CoreLocal {
	/// ID of the current Core.
	core_id: CoreId,
	/// Scheduler of the current Core.
	scheduler: Cell<*mut PerCoreScheduler>,
	/// Start address of the kernel stack
	pub kernel_stack: Cell<u64>,
	/// The core-local async executor.
	ex: StaticLocalExecutor<RawSpinMutex, RawRwSpinLock>,
	/// Queues to handle incoming requests from the other cores
	#[cfg(feature = "smp")]
	pub scheduler_input: InterruptTicketMutex<SchedulerInput>,
	#[cfg(not(feature = "riscv-plic"))]
	msi_controller: Cell<*mut MsiController>,
}

impl CoreLocal {
	pub fn install() {
		unsafe {
			let raw: *const Self;
			asm!("mv {}, gp", out(reg) raw);
			debug_assert_eq!(raw, ptr::null());

			let core_id = CPU_ONLINE.load(Ordering::Relaxed);

			let this = Self {
				core_id,
				scheduler: Cell::new(ptr::null_mut()),
				kernel_stack: Cell::new(0),
				ex: StaticLocalExecutor::new(),
				#[cfg(feature = "smp")]
				scheduler_input: InterruptTicketMutex::new(SchedulerInput::new()),
				#[cfg(not(feature = "riscv-plic"))]
				msi_controller: Cell::new(ptr::null_mut()),
			};
			let this = if core_id == 0 {
				take_static::take_static! {
					static FIRST_CORE_LOCAL: Option<CoreLocal> = None;
				}
				FIRST_CORE_LOCAL.take().unwrap().insert(this)
			} else {
				Box::leak(Box::new(this))
			};

			asm!("mv gp, {}", in(reg) this);
		}
	}

	#[inline]
	pub fn get() -> &'static Self {
		unsafe {
			let raw: *const Self;
			asm!("mv {}, gp", out(reg) raw);
			debug_assert_ne!(raw, ptr::null());
			&*raw
		}
	}
}

#[inline]
pub fn core_id() -> CoreId {
	CoreLocal::get().core_id
}

#[inline]
pub fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { CoreLocal::get().scheduler.get().as_mut().unwrap() }
}

#[inline]
pub fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	CoreLocal::get().scheduler.set(scheduler);
}

pub(crate) fn ex() -> &'static StaticLocalExecutor<RawSpinMutex, RawRwSpinLock> {
	&CoreLocal::get().ex
}

#[cfg(not(feature = "riscv-plic"))]
pub(crate) fn msi_controller() -> Option<&'static mut MsiController> {
	unsafe { CoreLocal::get().msi_controller.get().as_mut() }
}

#[cfg(not(feature = "riscv-plic"))]
pub(crate) fn set_msi_controller(msi_controller: *mut MsiController) {
	CoreLocal::get().msi_controller.set(msi_controller);
}
