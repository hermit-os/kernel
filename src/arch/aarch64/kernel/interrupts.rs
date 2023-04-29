use core::arch::asm;

use aarch64::regs::*;
use hermit_dtb::Dtb;
use hermit_sync::{InterruptTicketMutex, OnceCell};
use tock_registers::interfaces::Readable;

use crate::arch::aarch64::kernel::boot_info;
use crate::arch::aarch64::kernel::scheduler::State;
use crate::arch::aarch64::mm::paging::{
	self, virt_to_phys, BasePageSize, PageSize, PageTableEntryFlags,
};
use crate::arch::aarch64::mm::{virtualmem, PhysAddr, VirtAddr};
use crate::errno::EFAULT;
use crate::scheduler::CoreId;
use crate::sys_exit;

pub const IST_SIZE: usize = 8 * BasePageSize::SIZE as usize;

/*
 * GIC Distributor interface register offsets that are common to GICv3 & GICv2
 */

const GICD_CTLR: usize = 0x0;
const GICD_TYPER: usize = 0x4;
const GICD_IIDR: usize = 0x8;
const GICD_IGROUPR: usize = 0x80;
const GICD_ISENABLER: usize = 0x100;
const GICD_ICENABLER: usize = 0x180;
const GICD_ISPENDR: usize = 0x200;
const GICD_ICPENDR: usize = 0x280;
const GICD_ISACTIVER: usize = 0x300;
const GICD_ICACTIVER: usize = 0x380;
const GICD_IPRIORITYR: usize = 0x400;
const GICD_ITARGETSR: usize = 0x800;
const GICD_ICFGR: usize = 0xC00;
const GICD_NSACR: usize = 0xE00;
const GICD_SGIR: usize = 0xF00;

const GICD_CTLR_ENABLEGRP0: u32 = 1 << 0;
const GICD_CTLR_ENABLEGRP1: u32 = 1 << 1;

/* Physical CPU Interface registers */
const GICC_CTLR: usize = 0x0;
const GICC_PMR: usize = 0x4;
const GICC_BPR: usize = 0x8;
const GICC_IAR: usize = 0xC;
const GICC_EOIR: usize = 0x10;
const GICC_RPR: usize = 0x14;
const GICC_HPPIR: usize = 0x18;
const GICC_AHPPIR: usize = 0x28;
const GICC_IIDR: usize = 0xFC;
const GICC_DIR: usize = 0x1000;
const GICC_PRIODROP: usize = GICC_EOIR;

const GICC_CTLR_ENABLEGRP0: u32 = 1 << 0;
const GICC_CTLR_ENABLEGRP1: u32 = 1 << 1;
const GICC_CTLR_FIQEN: u32 = 1 << 3;
const GICC_CTLR_ACKCTL: u32 = 1 << 2;

/// maximum number of interrupt handlers
const MAX_HANDLERS: usize = 256;

static GICC_ADDRESS: OnceCell<VirtAddr> = OnceCell::new();
static GICD_ADDRESS: OnceCell<VirtAddr> = OnceCell::new();

/// Number of used supported interrupts
static NR_IRQS: OnceCell<u32> = OnceCell::new();
static mut INTERRUPT_HANDLERS: [fn(state: &State); MAX_HANDLERS] =
	[default_interrupt_handler; MAX_HANDLERS];

fn default_interrupt_handler(_state: &State) {
	warn!("Entering default interrupt handler");
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
	info!("Install handler for interrupt {}", irq_number);
	unsafe {
		INTERRUPT_HANDLERS[irq_number as usize] = handler;
	}
}

#[no_mangle]
pub extern "C" fn do_fiq(state: &State) {
	info!("fiq");
	let iar = gicc_read(GICC_IAR);
	let vector: usize = iar as usize & 0x3ff;

	info!("Receive fiq {}", vector);

	if vector < MAX_HANDLERS {
		unsafe {
			INTERRUPT_HANDLERS[vector](state);
		}
	}

	gicc_write(GICC_EOIR, iar.try_into().unwrap());
}

#[no_mangle]
pub extern "C" fn do_irq(_state: &State) {
	let iar = gicc_read(GICC_IAR);
	let vector = iar & 0x3ff;

	info!("Receive interrupt {}", vector);

	gicc_write(GICC_EOIR, iar);
}

#[no_mangle]
pub extern "C" fn do_sync(state: &State) {
	info!("{:#012x?}", state);
	let iar = gicc_read(GICC_IAR);
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

			// send EOI
			gicc_write(GICC_EOIR, iar);
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
	error!("Receive unhandled exception: {}\n", reason);

	sys_exit(-EFAULT);
}

#[no_mangle]
pub extern "C" fn do_error(_state: &State) -> ! {
	error!("Receive error interrupt\n");

	sys_exit(-EFAULT);
}

#[inline]
fn gicd_read(off: usize) -> u32 {
	let value: u32;

	// we have to use inline assembly to guarantee 32bit memory access
	unsafe {
		asm!("ldar {value:w}, [{addr}]",
			value = out(reg) value,
			addr = in(reg) (GICD_ADDRESS.get().unwrap().as_usize() + off),
			options(nostack, readonly),
		);
	}

	value
}

#[inline]
fn gicd_write(off: usize, value: u32) {
	// we have to use inline assembly to guarantee 32bit memory access
	unsafe {
		asm!("str {value:w}, [{addr}]",
			value = in(reg) value,
			addr = in(reg) (GICD_ADDRESS.get().unwrap().as_usize() + off),
			options(nostack),
		);
	}
}

#[inline]
fn gicc_read(off: usize) -> u32 {
	let value: u32;

	// we have to use inline assembly to guarantee 32bit memory access
	unsafe {
		asm!("ldar {value:w}, [{addr}]",
			value = out(reg) value,
			addr = in(reg) (GICC_ADDRESS.get().unwrap().as_usize() + off),
			options(nostack, readonly),
		);
	}

	value
}

#[inline]
fn gicc_write(off: usize, value: u32) {
	// we have to use inline assembly to guarantee 32bit memory access
	unsafe {
		asm!("str {value:w}, [{addr}]",
			value = in(reg) value,
			addr = in(reg) (GICC_ADDRESS.get().unwrap().as_usize() + off),
			options(nostack),
		);
	}
}

/// Global enable forwarding interrupts from distributor to cpu interface
fn gicd_enable() {
	gicd_write(GICD_CTLR, GICD_CTLR_ENABLEGRP0 | GICD_CTLR_ENABLEGRP1);
}

/// Global disable forwarding interrupts from distributor to cpu interface
fn gicd_disable() {
	gicd_write(GICD_CTLR, 0);
}

/// Global enable signalling of interrupt from the cpu interface
fn gicc_enable() {
	gicc_write(
		GICC_CTLR,
		GICC_CTLR_ENABLEGRP0 | GICC_CTLR_ENABLEGRP1 | GICC_CTLR_FIQEN | GICC_CTLR_ACKCTL,
	);
}

/// Global disable signalling of interrupt from the cpu interface
fn gicc_disable() {
	gicc_write(GICC_CTLR, 0);
}

fn gicc_set_priority(priority: u32) {
	gicc_write(GICC_PMR, priority & 0xFF);
}

static MASK_LOCK: InterruptTicketMutex<()> = InterruptTicketMutex::new(());

pub fn mask_interrupt(vector: u32) -> Result<(), ()> {
	if vector < *NR_IRQS.get().unwrap() && vector < MAX_HANDLERS.try_into().unwrap() {
		let _guard = MASK_LOCK.lock();

		let regoff = GICD_ICENABLER + 4 * (vector as usize / 32);
		gicd_write(regoff, 1 << (vector % 32));

		Ok(())
	} else {
		Err(())
	}
}

pub fn unmask_interrupt(vector: u32) -> Result<(), ()> {
	if vector < *NR_IRQS.get().unwrap() && vector < MAX_HANDLERS.try_into().unwrap() {
		let _guard = MASK_LOCK.lock();

		let regoff = GICD_ISENABLER + 4 * (vector as usize / 32);
		gicd_write(regoff, 1 << (vector % 32));
		Ok(())
	} else {
		Err(())
	}
}

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	todo!("set_oneshot_timer stub");
}

pub fn wakeup_core(core_to_wakeup: CoreId) {
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
	GICD_ADDRESS.set(gicd_address).unwrap();
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
	GICC_ADDRESS.set(gicc_address).unwrap();
	debug!("Mapping generic interrupt controller to virtual address {gicc_address:p}",);
	paging::map::<BasePageSize>(
		gicc_address,
		gicc_start,
		(gicc_size / BasePageSize::SIZE).try_into().unwrap(),
		flags,
	);

	gicc_disable();
	gicd_disable();

	let nr_irqs = ((gicd_read(GICD_TYPER) & 0x1f) + 1) * 32;
	info!("Number of supported interrupts {}", nr_irqs);
	NR_IRQS.set(nr_irqs).unwrap();

	gicd_write(GICD_ICENABLER, 0xffff0000);
	gicd_write(GICD_ISENABLER, 0x0000ffff);
	gicd_write(GICD_ICPENDR, 0xffffffff);
	gicd_write(GICD_IGROUPR, 0);

	for i in 0..32 / 4 {
		gicd_write(GICD_IPRIORITYR + i * 4, 0x80808080);
	}

	for i in 32 / 16..nr_irqs / 16 {
		gicd_write(GICD_NSACR + i as usize * 4, 0xffffffff);
	}

	for i in 32 / 32..nr_irqs / 32 {
		gicd_write(GICD_ICENABLER + i as usize * 4, 0xffffffff);
		gicd_write(GICD_ICPENDR + i as usize * 4, 0xffffffff);
		gicd_write(GICD_IGROUPR + i as usize * 4, 0);
	}

	for i in 32 / 4..nr_irqs / 4 {
		gicd_write(GICD_ITARGETSR + i as usize * 4, 0);
		gicd_write(GICD_IPRIORITYR + i as usize * 4, 0x80808080);
	}

	gicd_enable();

	gicc_set_priority(0xF0);
	gicc_enable();
}
