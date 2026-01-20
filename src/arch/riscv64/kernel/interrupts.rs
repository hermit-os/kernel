use alloc::vec::Vec;
use core::ptr;

use ahash::RandomState;
use hashbrown::HashMap;
use hermit_sync::{OnceCell, SpinMutex};
use riscv::asm::wfi;
use riscv::interrupt::{Exception, Interrupt, Trap};
use riscv::register::{scause, sie, sip, sstatus, stval};
use trapframe::TrapFrame;

use crate::drivers::InterruptHandlerQueue;
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_interrupt_handlers;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_interrupt_handlers;
use crate::scheduler;

/// The ID of the first Shared Peripheral Interrupt.
#[allow(dead_code)]
const SPI_START: u8 = 0;

/// base address of the PLIC, only one access at the same time is allowed
static PLIC_BASE: SpinMutex<usize> = SpinMutex::new(0x0);

/// PLIC context for new interrupt handlers
static PLIC_CONTEXT: SpinMutex<u16> = SpinMutex::new(0x0);

/// PLIC context for new interrupt handlers
static CURRENT_INTERRUPTS: SpinMutex<Vec<u32>> = SpinMutex::new(Vec::new());

static INTERRUPT_HANDLERS: OnceCell<HashMap<u8, InterruptHandlerQueue, RandomState>> =
	OnceCell::new();

/// Init Interrupts
pub(crate) fn install() {
	unsafe {
		// Install trap handler
		trapframe::init();
		// Enable external interrupts
		sie::set_sext();
	}
}

/// Init PLIC
pub(crate) fn init_plic(base: usize, context: u16) {
	*PLIC_BASE.lock() = base;
	*PLIC_CONTEXT.lock() = context;
}

/// Enable Interrupts
#[inline]
pub(crate) fn enable() {
	unsafe {
		sstatus::set_sie();
	}
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
				crate::arch::riscv64::kernel::scheduler::wakeup_handler();
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
				crate::arch::riscv64::kernel::scheduler::timer_handler();
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

/// Currently not needed because we use the trapframe crate
pub(crate) fn install_handlers() {
	let handlers = get_interrupt_handlers();

	for irq_number in handlers.keys() {
		unsafe {
			let base_ptr = PLIC_BASE.lock();
			let context = PLIC_CONTEXT.lock();

			// Set priority to 7 (highest on FU740)
			let prio_address = *base_ptr + *irq_number as usize * 4;
			let prio_ptr = ptr::with_exposed_provenance_mut::<u32>(prio_address);
			prio_ptr.write_volatile(1);
			// Set Threshold to 0 (lowest)
			let thresh_address = *base_ptr + 0x20_0000 + 0x1000 * (*context as usize);
			let thresh_ptr = ptr::with_exposed_provenance_mut::<u32>(thresh_address);
			thresh_ptr.write_volatile(0);
			// Enable irq for context
			const PLIC_ENABLE_OFFSET: usize = 0x0000_2000;
			let enable_address = *base_ptr
				+ PLIC_ENABLE_OFFSET
				+ 0x80 * (*context as usize)
				+ ((*irq_number / 32) * 4) as usize;
			let enable_ptr = ptr::with_exposed_provenance_mut::<u32>(enable_address);
			debug!("enable_address {enable_ptr:p}");
			enable_ptr.write_volatile(1 << (irq_number % 32));
		}
	}

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
			crate::arch::riscv64::kernel::scheduler::wakeup_handler();
		}
		Trap::Interrupt(Interrupt::SupervisorTimer) => {
			crate::arch::riscv64::kernel::scheduler::timer_handler();
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

	// Claim interrupt
	let base_ptr = PLIC_BASE.lock();
	let context = PLIC_CONTEXT.lock();
	let claim_address = *base_ptr + 0x20_0004 + 0x1000 * (*context as usize);
	let claim_ptr = ptr::with_exposed_provenance_mut::<u32>(claim_address);
	let irq = unsafe { claim_ptr.read_volatile() };

	if irq != 0 {
		debug!("External INT: {irq}");
		let mut cur_int = CURRENT_INTERRUPTS.lock();
		cur_int.push(irq);
		if cur_int.len() > 1 {
			warn!("More than one external interrupt is pending!");
		}
		// Release lock early
		drop(cur_int);

		// Call handler
		if let Some(handlers) = INTERRUPT_HANDLERS.get()
			&& let Some(queue) = handlers.get(&u8::try_from(irq).unwrap())
		{
			for handler in queue.iter() {
				handler();
			}
		}
		crate::executor::run();

		core_scheduler().reschedule();

		// Complete interrupt after handling
		unsafe {
			claim_ptr.write_volatile(irq);
		}

		// Remove from active interrupts
		let mut cur_int = CURRENT_INTERRUPTS.lock();
		if let Some(active_irq) = cur_int.pop()
			&& active_irq != irq
		{
			warn!("Interrupt mismatch during EOI!");
		}
	}
}

pub(crate) fn print_statistics() {}

#[path = "../../../kernel/interrupts.rs"]
mod interrupts_common;
pub(crate) use interrupts_common::*;
