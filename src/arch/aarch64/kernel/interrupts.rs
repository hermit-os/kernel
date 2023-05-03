use alloc::vec::Vec;
use core::arch::asm;
use core::ptr;

use aarch64::regs::*;
use arm_gic::gicv3::{GicV3, IntId, SgiTarget};
use hermit_dtb::Dtb;
use hermit_sync::{without_interrupts, InterruptTicketMutex, OnceCell};
use tock_registers::interfaces::Readable;

use crate::arch::aarch64::kernel::boot_info;
use crate::arch::aarch64::kernel::scheduler::State;
use crate::arch::aarch64::mm::paging::{
	self, virt_to_phys, BasePageSize, PageSize, PageTableEntryFlags,
};
use crate::arch::aarch64::mm::{virtualmem, PhysAddr, VirtAddr};
use crate::errno::EFAULT;
use crate::scheduler::CoreId;
use crate::{core_scheduler, sys_exit};

pub const IST_SIZE: usize = 8 * BasePageSize::SIZE as usize;

/// maximum number of interrupt handlers
const MAX_HANDLERS: usize = 256;

/// Number of the timer interrupt
static mut TIMER_INTERRUPT: u32 = 0;
/// Possible interrupt handlers
static mut INTERRUPT_HANDLERS: [Option<fn(state: &State)>; MAX_HANDLERS] = [None; MAX_HANDLERS];
/// Driver for the Arm Generic Interrupt Controller version 3 (or 4).
static mut GIC: OnceCell<GicV3> = OnceCell::new();

fn timer_handler(_state: &State) {
	debug!("Handle timer interrupt");

	// disable timer
	unsafe {
		asm!(
			"msr cntp_cval_el0, {disable}",
			"msr cntp_ctl_el0, {disable}",
			disable = in(reg) 0,
			options(nostack, nomem),
		);
	}
}

/// Enable all interrupts
#[inline]
pub fn enable() {
	unsafe {
		asm!(
			"msr daifclr, {mask}",
			mask = const 0b111,
			options(nostack, nomem),
		);
	}
}

/// Enable Interrupts and wait for the next interrupt (HLT instruction)
/// According to <https://lists.freebsd.org/pipermail/freebsd-current/2004-June/029369.html>, this exact sequence of assembly
/// instructions is guaranteed to be atomic.
/// This is important, because another CPU could call wakeup_core right when we decide to wait for the next interrupt.
#[inline]
pub fn enable_and_wait() {
	unsafe {
		asm!(
			"msr daifclr, {mask}; wfi",
			mask = const 0b111,
			options(nostack, nomem),
		);
	}
}

/// Disable all interrupts
#[inline]
pub fn disable() {
	unsafe {
		asm!(
			"msr daifset, {mask}",
			mask = const 0b111,
			options(nostack, nomem),
		);
	}
}

pub fn irq_install_handler(irq_number: u32, handler: fn(state: &State)) {
	debug!("Install handler for interrupt {}", irq_number);
	unsafe {
		INTERRUPT_HANDLERS[irq_number as usize] = Some(handler);
	}
}

#[no_mangle]
pub extern "C" fn do_fiq(state: &State) {
	if let Some(irqid) = GicV3::get_and_acknowledge_interrupt() {
		let vector: usize = u32::from(irqid).try_into().unwrap();

		debug!("Receive fiq {}", vector);

		if vector < MAX_HANDLERS {
			unsafe {
				if let Some(handler) = INTERRUPT_HANDLERS[vector] {
					handler(state);
				}
			}
		}

		core_scheduler().handle_waiting_tasks();

		GicV3::end_interrupt(irqid);

		if unsafe { vector == TIMER_INTERRUPT.try_into().unwrap() } {
			// a timer interrupt may have caused unblocking of tasks
			core_scheduler().scheduler();
		}
	}
}

#[no_mangle]
pub extern "C" fn do_irq(state: &State) {
	if let Some(irqid) = GicV3::get_and_acknowledge_interrupt() {
		let vector: usize = u32::from(irqid).try_into().unwrap();

		debug!("Receive interrupt {}", vector);

		if vector < MAX_HANDLERS {
			unsafe {
				if let Some(handler) = INTERRUPT_HANDLERS[vector] {
					handler(state);
				}
			}
		}

		core_scheduler().handle_waiting_tasks();

		GicV3::end_interrupt(irqid);

		if unsafe { vector == TIMER_INTERRUPT.try_into().unwrap() } {
			// a timer interrupt may have caused unblocking of tasks
			core_scheduler().scheduler();
		}
	}
}

#[no_mangle]
pub extern "C" fn do_sync(_state: &State) {
	let irqid = GicV3::get_and_acknowledge_interrupt().unwrap();
	let esr = ESR_EL1.get();
	let ec = esr >> 26;
	let iss = esr & 0xFFFFFF;
	let pc = ELR_EL1.get();

	/* data abort from lower or current level */
	if (ec == 0b100100) || (ec == 0b100101) {
		/* check if value in far_el1 is valid */
		if (iss & (1 << 10)) == 0 {
			/* read far_el1 register, which holds the faulting virtual address */
			let far = FAR_EL1.get();

			// add page fault handler

			error!("Unable to handle page fault at {:#x}", far);
			error!("Exception return address {:#x}", ELR_EL1.get());
			error!("Thread ID register {:#x}", TPIDR_EL0.get());
			error!("Table Base Register {:#x}", TTBR0_EL1.get());
			error!("Exception Syndrome Register {:#x}", esr);

			GicV3::end_interrupt(irqid);
			sys_exit(-EFAULT);
		} else {
			error!("Unknown exception");
		}
	} else if ec == 0x3c {
		error!("Trap to debugger, PC={:#x}", pc);
	} else {
		error!("Unsupported exception class: {:#x}, PC={:#x}", ec, pc);
	}
}

#[no_mangle]
pub extern "C" fn do_bad_mode(_state: &State, reason: u32) -> ! {
	error!("Receive unhandled exception: {}", reason);

	sys_exit(-EFAULT);
}

#[no_mangle]
pub extern "C" fn do_error(_state: &State) -> ! {
	error!("Receive error interrupt");

	sys_exit(-EFAULT);
}

pub fn wakeup_core(_core_to_wakeup: CoreId) {
	todo!("wakeup_core stub");
}

pub fn init() {
	info!("Intialize generic interrupt controller");

	let dtb = unsafe {
		Dtb::from_raw(boot_info().hardware_info.device_tree.unwrap().get() as *const u8)
			.expect(".dtb file has invalid header")
	};

	let reg = dtb.get_property("/intc", "reg").unwrap();
	let (slice, residual_slice) = reg.split_at(core::mem::size_of::<u64>());
	let gicd_start = PhysAddr(u64::from_be_bytes(slice.try_into().unwrap()));
	let (slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
	let gicd_size = u64::from_be_bytes(slice.try_into().unwrap());
	let (slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
	let gicc_start = PhysAddr(u64::from_be_bytes(slice.try_into().unwrap()));
	let (slice, _residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
	let gicc_size = u64::from_be_bytes(slice.try_into().unwrap());

	info!(
		"Found GIC Distributor interface at {:#X} (size {:#X})",
		gicd_start, gicd_size
	);
	info!(
		"Found generic interrupt controller at {:#X} (size {:#X})",
		gicc_start, gicc_size
	);

	let gicd_address =
		virtualmem::allocate_aligned(gicd_size.try_into().unwrap(), 0x10000).unwrap();
	debug!("Mapping GIC Distributor interface to virtual address {gicd_address:p}",);

	let mut flags = PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	paging::map::<BasePageSize>(
		gicd_address,
		gicd_start,
		(gicd_size / BasePageSize::SIZE).try_into().unwrap(),
		flags,
	);

	let gicc_address =
		virtualmem::allocate_aligned(gicc_size.try_into().unwrap(), 0x10000).unwrap();
	debug!("Mapping generic interrupt controller to virtual address {gicc_address:p}",);
	paging::map::<BasePageSize>(
		gicc_address,
		gicc_start,
		(gicc_size / BasePageSize::SIZE).try_into().unwrap(),
		flags,
	);

	GicV3::set_priority_mask(0xff);
	let mut gic = unsafe { GicV3::new(gicd_address.as_mut_ptr(), gicc_address.as_mut_ptr()) };
	gic.setup();

	for node in dtb.enum_subnodes("/") {
		let parts: Vec<_> = node.split('@').collect();

		if let Some(compatible) = dtb.get_property(parts.first().unwrap(), "compatible") {
			if core::str::from_utf8(compatible)
				.unwrap()
				.find("timer")
				.is_some()
			{
				let irq_slice = dtb
					.get_property(parts.first().unwrap(), "interrupts")
					.unwrap();
				/* Secure Phys IRQ */
				let (_irqtype, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
				let (_irq, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
				let (_irqflags, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
				/* Non-secure Phys IRQ */
				let (irqtype, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
				let (irq, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
				let (irqflags, _irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
				let _irqtype = u32::from_be_bytes(irqtype.try_into().unwrap());
				let irq = u32::from_be_bytes(irq.try_into().unwrap());
				let _irqflags = u32::from_be_bytes(irqflags.try_into().unwrap());
				unsafe {
					TIMER_INTERRUPT = irq;
				}

				info!("Timer interrupt: {}", irq);
				irq_install_handler(irq + 16, timer_handler);

				// enable timer interrupt
				let timer_irqid = IntId::ppi(irq);
				gic.set_interrupt_priority(timer_irqid, 0x00);
				gic.enable_interrupt(timer_irqid, true);
			}
		}
	}

	unsafe {
		GIC.set(gic).unwrap();
	}
}
