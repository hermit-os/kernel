use alloc::boxed::Box;
use alloc::collections::vec_deque::VecDeque;
use core::arch::asm;
use core::cell::{Cell, RefCell, RefMut};
use core::ptr;
use core::sync::atomic::Ordering;

#[cfg(feature = "smp")]
use hermit_sync::InterruptTicketMutex;

use super::CPU_ONLINE;
use super::interrupts::{IRQ_COUNTERS, IrqStatistics};
use crate::executor::task::AsyncTask;
#[cfg(feature = "smp")]
use crate::scheduler::SchedulerInput;
use crate::scheduler::{CoreId, PerCoreScheduler};

pub(crate) struct CoreLocal {
	this: *const Self,
	/// ID of the current Core.
	core_id: CoreId,
	/// Scheduler of the current Core.
	scheduler: Cell<*mut PerCoreScheduler>,
	/// Interface to the interrupt counters
	irq_statistics: &'static IrqStatistics,
	/// Queue of async tasks
	async_tasks: RefCell<VecDeque<AsyncTask>>,
	/// Queues to handle incoming requests from the other cores
	#[cfg(feature = "smp")]
	pub scheduler_input: InterruptTicketMutex<SchedulerInput>,
}

impl CoreLocal {
	pub fn install() {
		let core_id = CPU_ONLINE.0.load(Ordering::Relaxed);

		let irq_statistics = if core_id == 0 {
			static FIRST_IRQ_STATISTICS: IrqStatistics = IrqStatistics::new();
			&FIRST_IRQ_STATISTICS
		} else {
			&*Box::leak(Box::new(IrqStatistics::new()))
		};

		let this = Self {
			this: ptr::null_mut(),
			core_id,
			scheduler: Cell::new(ptr::null_mut()),
			irq_statistics,
			async_tasks: RefCell::new(VecDeque::new()),
			#[cfg(feature = "smp")]
			scheduler_input: InterruptTicketMutex::new(SchedulerInput::new()),
		};
		let this = if core_id == 0 {
			take_static::take_static! {
				static FIRST_CORE_LOCAL: Option<CoreLocal> = None;
			}
			FIRST_CORE_LOCAL.take().unwrap().insert(this)
		} else {
			this.add_irq_counter();
			Box::leak(Box::new(this))
		};
		this.this = ptr::from_ref(this);

		unsafe {
			asm!("msr tpidr_el1, {}", in(reg) this, options(nostack, preserves_flags));
		}
	}

	#[inline]
	pub fn get() -> &'static Self {
		unsafe {
			let raw: *const Self;
			asm!("mrs {}, tpidr_el1", out(reg) raw, options(nomem, nostack, preserves_flags));
			&*raw
		}
	}

	pub fn add_irq_counter(&self) {
		IRQ_COUNTERS
			.lock()
			.insert(self.core_id, self.irq_statistics);
	}
}

#[inline]
pub(crate) fn core_id() -> CoreId {
	if cfg!(target_os = "none") {
		CoreLocal::get().core_id
	} else {
		0
	}
}

#[inline]
pub(crate) fn core_scheduler() -> &'static mut PerCoreScheduler {
	unsafe { CoreLocal::get().scheduler.get().as_mut().unwrap() }
}

pub(crate) fn async_tasks() -> RefMut<'static, VecDeque<AsyncTask>> {
	CoreLocal::get().async_tasks.borrow_mut()
}

pub(crate) fn set_core_scheduler(scheduler: *mut PerCoreScheduler) {
	CoreLocal::get().scheduler.set(scheduler);
}

pub(crate) fn increment_irq_counter(irq_no: u8) {
	CoreLocal::get().irq_statistics.inc(irq_no);
}
