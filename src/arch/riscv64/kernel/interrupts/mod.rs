use core::num::NonZeroU16;

use ahash::RandomState;
use hashbrown::HashMap;
use hermit_sync::{InterruptTicketMutex, OnceCell, SpinMutex};
use riscv::asm::wfi;
use riscv::interrupt::{Exception, Interrupt, Trap};
use riscv::register::{scause, sie, sip, sstatus, stval};
use trapframe::TrapFrame;

use crate::arch::kernel::devicetree::InterruptType as DeviceTreeInterruptType;
use crate::drivers::InterruptHandlerMap;
use crate::scheduler;

#[cfg(not(feature = "riscv-plic"))]
mod aplic;
#[cfg(not(feature = "riscv-plic"))]
pub(crate) use aplic::init_aplic;
#[cfg(not(feature = "riscv-plic"))]
use aplic::{Aplic, SourceMode};

#[cfg(feature = "riscv-plic")]
mod plic;
#[cfg(feature = "riscv-plic")]
use plic::Plic;
#[cfg(feature = "riscv-plic")]
pub(crate) use plic::init_plic;

pub(crate) static EXTERNAL_INTERRUPT_CONTROLLER: SpinMutex<Option<ExternalInterruptController>> =
	SpinMutex::new(None);

static INTERRUPT_HANDLERS: OnceCell<InterruptHandlerMap> = OnceCell::new();

pub(crate) enum ExternalInterruptController {
	#[cfg(feature = "riscv-plic")]
	Plic(Plic),
	#[cfg(not(feature = "riscv-plic"))]
	Aplic(Aplic),
}

impl ExternalInterruptController {
	fn enable_interrupt(&mut self, irq_number: u16) {
		match self {
			#[cfg(feature = "riscv-plic")]
			Self::Plic(plic) => plic.set_enable_bit(irq_number, true),
			#[cfg(not(feature = "riscv-plic"))]
			Self::Aplic(aplic) => aplic.set_enable_bit(irq_number, true),
		}
	}

	#[cfg_attr(not(any(feature = "virtio", feature = "pci")), allow(dead_code))]
	pub fn set_interrupt_source_mode(
		&mut self,
		_irq_number: u16,
		_irq_type: DeviceTreeInterruptType,
	) {
		match self {
			#[cfg(feature = "riscv-plic")]
			Self::Plic(_plic) => { /* noop */ }
			#[cfg(not(feature = "riscv-plic"))]
			Self::Aplic(aplic) => {
				aplic.set_interrupt_source_mode(_irq_number, SourceMode::from(_irq_type));
			}
		}
	}

	fn set_interrupt_priority(&mut self, irq_number: u16, priority: u8) {
		match self {
			#[cfg(feature = "riscv-plic")]
			Self::Plic(plic) => plic.set_interrupt_priority(irq_number, priority),
			#[cfg(not(feature = "riscv-plic"))]
			Self::Aplic(aplic) => aplic.set_interrupt_priority(irq_number, priority),
		}
	}

	fn set_priority_threshold(&mut self, threshold: u8) {
		match self {
			#[cfg(feature = "riscv-plic")]
			Self::Plic(plic) => plic.set_priority_threshold(threshold),
			#[cfg(not(feature = "riscv-plic"))]
			Self::Aplic(aplic) => aplic.set_priority_threshold(threshold),
		}
	}

	fn claim_interrupt(&mut self) -> Option<NonZeroU16> {
		match self {
			#[cfg(feature = "riscv-plic")]
			Self::Plic(plic) => plic.claim_interrupt(),
			#[cfg(not(feature = "riscv-plic"))]
			Self::Aplic(aplic) => aplic.claim_interrupt(),
		}
	}

	fn complete_interrupt(&mut self, irq_number: u16) {
		match self {
			#[cfg(feature = "riscv-plic")]
			Self::Plic(plic) => plic.complete_interrupt(irq_number),
			#[cfg(not(feature = "riscv-plic"))]
			Self::Aplic(aplic) => aplic.complete_interrupt(irq_number),
		}
	}
}

/// Init Interrupts
pub(crate) fn install() {
	unsafe {
		// Install trap handler
		trapframe::init();
		// Enable external interrupts
		sie::set_sext();
	}
}

/// Enable Interrupts
#[inline]
pub(crate) fn enable() {
	unsafe {
		sstatus::set_sie();
	}
}

static IRQ_NAMES: InterruptTicketMutex<HashMap<u8, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

#[allow(dead_code)]
pub(crate) fn add_irq_name(irq_number: u8, name: &'static str) {
	debug!("Register name \"{name}\" for interrupt {irq_number}");
	IRQ_NAMES.lock().insert(irq_number, name);
}

/// Waits for the next interrupt (Only Supervisor-level software/timer interrupt for now)
/// and calls the specific handler
#[inline]
pub(crate) fn enable_and_wait() {
	unsafe {
		//Enable Supervisor-level software interrupts
		sie::set_ssoft();
		//sie::set_sext();
		debug!("Wait {:x?}", sie::read());
		loop {
			wfi();
			// Interrupts are disabled at this point, so a pending interrupt will
			// resume the execution. We still have to check if a interrupt is pending
			// because the WFI instruction could be implemented as NOP (The RISC-V Instruction Set ManualVolume II: Privileged Architecture)

			let pending_interrupts = sip::read();

			// trace!("sip: {:x?}", pending_interrupts);
			#[cfg(feature = "smp")]
			if pending_interrupts.ssoft() {
				//Clear Supervisor-level software interrupt
				core::arch::asm!(
					"csrc sip, {ssoft_mask}",
					ssoft_mask = in(reg) 0x2,
				);
				trace!("SOFT");
				//Disable Supervisor-level software interrupt
				sie::clear_ssoft();
				crate::arch::kernel::scheduler::wakeup_handler();
				break;
			}

			if pending_interrupts.sext() {
				trace!("EXT");
				external_handler();
				break;
			}

			if pending_interrupts.stimer() {
				// // Disable Supervisor-level software interrupt, wakeup not needed
				// sie::clear_ssoft();

				debug!("sip: {pending_interrupts:x?}");
				trace!("TIMER");
				crate::arch::kernel::scheduler::timer_handler();
				break;
			}
		}
	}
}

/// Disable Interrupts
#[inline]
pub(crate) fn disable() {
	unsafe { sstatus::clear_sie() };
}

pub(crate) fn install_handlers(handlers: InterruptHandlerMap) {
	let mut ctrl_guard = EXTERNAL_INTERRUPT_CONTROLLER.lock();
	let ctrl = ctrl_guard.as_mut().unwrap();

	for irq_number in handlers.keys() {
		// Set priority to 255 (lowest priority)
		ctrl.set_interrupt_priority(u16::from(*irq_number), u8::MAX);
		ctrl.enable_interrupt(u16::from(*irq_number));
	}
	ctrl.set_priority_threshold(0);

	INTERRUPT_HANDLERS.set(handlers).unwrap();
}

// Derived from rCore: https://github.com/rcore-os/rCore
/// Dispatch and handle interrupt.
///
/// This function is called from `trap.S` which is in the trapframe crate.
#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(tf: &mut TrapFrame) {
	let scause = scause::read();
	let cause = scause.cause();
	let cause = Trap::<Interrupt, Exception>::try_from(cause).unwrap();
	let stval = stval::read();
	let sepc = tf.sepc;
	trace!("Interrupt: {cause:?}");
	trace!("tf = {tf:x?} ");
	trace!("stval = {stval:x}");
	trace!("sepc = {sepc:x}");
	trace!("SSTATUS FS = {:?}", sstatus::read().fs());

	match cause {
		Trap::Interrupt(Interrupt::SupervisorExternal) => external_handler(),
		#[cfg(feature = "smp")]
		Trap::Interrupt(Interrupt::SupervisorSoft) => {
			crate::arch::kernel::scheduler::wakeup_handler();
		}
		Trap::Interrupt(Interrupt::SupervisorTimer) => {
			crate::arch::kernel::scheduler::timer_handler();
		}
		cause => {
			error!("Interrupt: {cause:?}");
			error!("tf = {tf:x?} ");
			error!("stval = {stval:x}");
			error!("sepc = {sepc:x}");
			error!("SSTATUS FS = {:?}", sstatus::read().fs());
			scheduler::abort();
		}
	}
	trace!("Interrupt end");
}

/// Handles external interrupts
fn external_handler() {
	use crate::arch::kernel::core_local::core_scheduler;
	use crate::scheduler::PerCoreSchedulerExt;

	let irq = EXTERNAL_INTERRUPT_CONTROLLER
		.lock()
		.as_mut()
		.unwrap()
		.claim_interrupt();
	let Some(irq) = irq else { return };

	let irq_number = irq.get();
	debug!("External INT: {irq_number}");
	if let Some(handlers) = INTERRUPT_HANDLERS.get()
		&& let Ok(irq_u8) = u8::try_from(irq_number)
		&& let Some(queue) = handlers.get(&irq_u8)
	{
		for handler in queue.iter() {
			handler();
		}
	}

	crate::executor::run();

	EXTERNAL_INTERRUPT_CONTROLLER
		.lock()
		.as_mut()
		.unwrap()
		.complete_interrupt(irq_number);

	core_scheduler().reschedule();
}

pub(crate) fn print_statistics() {}
