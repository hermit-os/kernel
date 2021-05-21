use alloc::vec::Vec;
use core::arch::asm;
use riscv::asm::wfi;
use riscv::register::*;
use trapframe::TrapFrame;

use crate::synch::spinlock::Spinlock;

/// base address of the PLIC, only one access at the same time is allowed
static PLIC_BASE: Spinlock<usize> = Spinlock::new(0x0);

/// PLIC context for new interrupt handlers
static PLIC_CONTEXT: Spinlock<u16> = Spinlock::new(0x0);

/// PLIC context for new interrupt handlers
static CURRENT_INTERRUPTS: Spinlock<Vec<u32>> = Spinlock::new(Vec::new());

const PLIC_PENDING_OFFSET: usize = 0x001000;
const PLIC_ENABLE_OFFSET: usize = 0x002000;

const MAX_IRQ: usize = 69;

static mut IRQ_HANDLERS: [usize; MAX_IRQ] = [0; MAX_IRQ];

/// Init Interrupts
pub fn install() {
	unsafe {
		// Intstall trap handler
		trapframe::init();
		// Enable external interrupts
		sie::set_sext();
	}
}

/// Init PLIC
pub fn init_plic(base: usize, context: u16) {
	*PLIC_BASE.lock() = base;
	*PLIC_CONTEXT.lock() = context;
}

/// Enable Interrupts
#[inline]
pub fn enable() {
	unsafe {
		sstatus::set_sie();
	}
}

/// Waits for the next interrupt (Only Supervisor-level software/timer interrupt for now)
/// and calls the specific handler
#[inline]
pub fn enable_and_wait() {
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
				asm!(
					"csrc sip, {ssoft_mask}",
					ssoft_mask = in(reg) 0x2,
				);
				trace!("SOFT");
				//Disable Supervisor-level software interrupt
				sie::clear_ssoft();
				crate::arch::riscv::kernel::scheduler::wakeup_handler();
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

				debug!("sip: {:x?}", pending_interrupts);
				trace!("TIMER");
				crate::arch::riscv::kernel::scheduler::timer_handler();
				break;
			}
		}
	}
}

/// Disable Interrupts
#[inline]
pub fn disable() {
	unsafe { sstatus::clear_sie() };
}

/// Disable IRQs (nested)
///
/// Disable IRQs when unsure if IRQs were enabled at all.
/// This function together with nested_enable can be used
/// in situations when interrupts shouldn't be activated if they
/// were not activated before calling this function.
#[inline]
pub fn nested_disable() -> bool {
	let was_enabled = sstatus::read().sie();

	disable();
	was_enabled
}

/// Enable IRQs (nested)
///
/// Can be used in conjunction with nested_disable() to only enable
/// interrupts again if they were enabled before.
#[inline]
pub fn nested_enable(was_enabled: bool) {
	if was_enabled {
		enable();
	}
}

/// Currently not needed because we use the trapframe crate
#[no_mangle]
pub extern "C" fn irq_install_handler(irq_number: u32, handler: usize) {
	unsafe {
		let base_ptr = PLIC_BASE.lock();
		let context = PLIC_CONTEXT.lock();
		debug!(
			"Install handler for interrupt {}, context {}",
			irq_number, *context
		);
		IRQ_HANDLERS[irq_number as usize - 1] = handler;
		// Set priority to 7 (highest on FU740)
		let prio_address = *base_ptr + irq_number as usize * 4;
		core::ptr::write_volatile(prio_address as *mut u32, 1);
		// Set Threshold to 0 (lowest)
		let thresh_address = *base_ptr + 0x20_0000 + 0x1000 * (*context as usize);
		core::ptr::write_volatile(thresh_address as *mut u32, 0);
		// Enable irq for context
		let enable_address = *base_ptr
			+ PLIC_ENABLE_OFFSET
			+ 0x80 * (*context as usize)
			+ ((irq_number / 32) * 4) as usize;
		debug!("enable_address {:x}", enable_address);
		core::ptr::write_volatile(enable_address as *mut u32, 1 << (irq_number % 32));
	}
}

// Derived from rCore: https://github.com/rcore-os/rCore
/// Dispatch and handle interrupt.
///
/// This function is called from `trap.S` which is in the trapframe crate.
#[no_mangle]
pub extern "C" fn trap_handler(tf: &mut TrapFrame) {
	use self::scause::{Exception as E, Interrupt as I, Trap};
	let scause = scause::read();
	let stval = stval::read();
	let sepc = tf.sepc;
	trace!("Interrupt: {:?} ", scause.cause());
	trace!("tf: {:x?} ", tf);
	trace!("stvall: {:x}", stval);
	trace!("sepc: {:x}", sepc);
	trace!("SSTATUS FS: {:?}", sstatus::read().fs());
	trace!("FCSR: {:x?}", fcsr::read());
	//loop{}
	match scause.cause() {
		Trap::Interrupt(I::SupervisorExternal) => external_handler(),
		#[cfg(feature = "smp")]
		Trap::Interrupt(I::SupervisorSoft) => crate::arch::riscv::kernel::scheduler::wakeup_handler(),
		Trap::Interrupt(I::SupervisorTimer) => {
			crate::arch::riscv::kernel::scheduler::timer_handler()
		}
		//Trap::Exception(E::LoadPageFault) => page_fault(stval, tf),
		//Trap::Exception(E::StorePageFault) => page_fault(stval, tf),
		//Trap::Exception(E::InstructionPageFault) => page_fault(stval, tf),
		_ => {
			error!("Interrupt: {:?} ", scause.cause());
			error!("tf: {:x?} ", tf);
			error!("stvall: {:x}", stval);
			error!("sepc: {:x}", sepc);
			error!("SSTATUS FS: {:?}", sstatus::read().fs());
			error!("FCSR: {:x?}", fcsr::read());
			panic!("unhandled trap {:?}", scause.cause())
		}
	}
	trace!("Interrupt end");
}

/// Handles external interrupts
fn external_handler() {
	unsafe {
		let handler: Option<fn()> = {
			// Claim interrupt
			let base_ptr = PLIC_BASE.lock();
			let context = PLIC_CONTEXT.lock();
			//let claim_address = *base_ptr + 0x20_2004;
			let claim_address = *base_ptr + 0x20_0004 + 0x1000 * (*context as usize);
			let irq = core::ptr::read_volatile(claim_address as *mut u32);
			if irq != 0 {
				debug!("External INT: {}", irq);
				let mut cur_int = CURRENT_INTERRUPTS.lock();
				cur_int.push(irq);
				if cur_int.len() > 1 {
					warn!("More than one external interrupt is pending!");
				}

				// Call handler
				if IRQ_HANDLERS[irq as usize - 1] != 0 {
					let ptr = IRQ_HANDLERS[irq as usize - 1] as *const ();
					let handler: fn() = core::mem::transmute(ptr);
					Some(handler)
				} else {
					error!("Interrupt handler not installed");
					None
				}
			} else {
				None
			}
		};

		if let Some(handler) = handler {
			handler();
		}
	}
}

/// End of external interrupt
pub fn external_eoi() {
	unsafe {
		let base_ptr = PLIC_BASE.lock();
		let context = PLIC_CONTEXT.lock();
		let claim_address = *base_ptr + 0x20_0004 + 0x1000 * (*context as usize);

		let mut cur_int = CURRENT_INTERRUPTS.lock();
		let irq = cur_int.pop().unwrap_or(0);
		if irq != 0 {
			debug!("EOI INT: {}", irq);
			// Complete interrupt
			core::ptr::write_volatile(claim_address as *mut u32, irq);
		} else {
			warn!("Called EOI without active interrupt");
		}
	}
}
