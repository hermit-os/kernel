use alloc::collections::{BTreeMap, VecDeque};
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use aarch64_cpu::asm::barrier::{ISH, SY, dmb, isb};
use aarch64_cpu::registers::*;
use ahash::RandomState;
use arm_gic::gicv3::{GicV3, InterruptGroup, SgiTarget, SgiTargetGroup};
use arm_gic::{IntId, Trigger};
use fdt::standard_nodes::Compatible;
use free_list::PageLayout;
use hashbrown::HashMap;
use hermit_sync::{InterruptSpinMutex, OnceCell, SpinMutex};
use memory_addresses::VirtAddr;
use memory_addresses::arch::aarch64::PhysAddr;

use crate::arch::aarch64::kernel::core_local::increment_irq_counter;
use crate::arch::aarch64::kernel::scheduler::State;
use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_interrupt_handlers;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_interrupt_handlers;
use crate::drivers::{InterruptHandlerQueue, InterruptLine};
use crate::kernel::serial::handle_uart_interrupt;
use crate::mm::{PageAlloc, PageRangeAllocator};
use crate::scheduler::{self, CoreId};
use crate::{core_id, core_scheduler, env};

/// The ID of the first Private Peripheral Interrupt.
const PPI_START: u8 = 16;
/// The ID of the first Shared Peripheral Interrupt.
#[allow(dead_code)]
const SPI_START: u8 = 32;
/// Software-generated interrupt for rescheduling
pub(crate) const SGI_RESCHED: u8 = 1;

/// Number of the timer interrupt
static mut TIMER_INTERRUPT: u32 = 0;
/// Number of the UART interrupt
static mut UART_INTERRUPT: u32 = 0;
/// Possible interrupt handlers
static INTERRUPT_HANDLERS: OnceCell<HashMap<u8, InterruptHandlerQueue, RandomState>> =
	OnceCell::new();
/// Driver for the Arm Generic Interrupt Controller version 3 (or 4).
pub(crate) static GIC: SpinMutex<Option<GicV3<'_>>> = SpinMutex::new(None);

/// Enable all interrupts
#[inline]
pub fn enable() {
	dmb(ISH);
	unsafe {
		asm!(
			"msr daifclr, {mask}",
			mask = const 0b111,
			options(nostack),
		);
	}
	dmb(ISH);
}

/// Enable all interrupts and wait for the next interrupt (wfi instruction)
#[inline]
pub fn enable_and_wait() {
	dmb(ISH);
	unsafe {
		asm!(
			"msr daifclr, {mask}; wfi",
			mask = const 0b111,
			options(nostack),
		);
	}
	dmb(ISH);
}

/// Disable all interrupts
#[inline]
pub fn disable() {
	dmb(ISH);
	unsafe {
		asm!(
			"msr daifset, {mask}",
			mask = const 0b111,
			options(nostack),
		);
	}
	dmb(ISH);
}

pub(crate) fn install_handlers() {
	let mut handlers: HashMap<InterruptLine, InterruptHandlerQueue, RandomState> =
		HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0));

	fn timer_handler() {
		debug!("Handle timer interrupt");

		// disable timer
		CNTP_CVAL_EL0.set(0);
		CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::CLEAR);
	}

	for (key, value) in get_interrupt_handlers().into_iter() {
		handlers.insert(key + SPI_START, value);
	}

	unsafe {
		if let Some(queue) = handlers.get_mut(&(u8::try_from(TIMER_INTERRUPT).unwrap() + PPI_START))
		{
			queue.push_back(timer_handler);
		} else {
			let mut queue = VecDeque::<fn()>::new();
			queue.push_back(timer_handler);
			handlers.insert(u8::try_from(TIMER_INTERRUPT).unwrap() + PPI_START, queue);
		}

		if let Some(queue) = handlers.get_mut(&(u8::try_from(UART_INTERRUPT).unwrap() + SPI_START))
		{
			queue.push_back(handle_uart_interrupt);
		} else {
			let mut queue = VecDeque::<fn()>::new();
			queue.push_back(handle_uart_interrupt);
			handlers.insert(u8::try_from(UART_INTERRUPT).unwrap() + SPI_START, queue);
		}
	}

	INTERRUPT_HANDLERS.set(handlers).unwrap();
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_fiq(_state: &State) -> *mut usize {
	if let Some(irqid) = GicV3::get_and_acknowledge_interrupt(InterruptGroup::Group1) {
		let vector: u8 = u32::from(irqid).try_into().unwrap();

		debug!("Receive fiq {vector}");
		increment_irq_counter(vector);

		if let Some(handlers) = INTERRUPT_HANDLERS.get()
			&& let Some(queue) = handlers.get(&vector)
		{
			for handler in queue.iter() {
				handler();
			}
		}
		crate::executor::run();
		core_scheduler().handle_waiting_tasks();

		GicV3::end_interrupt(irqid, InterruptGroup::Group1);

		return core_scheduler().scheduler().unwrap_or_default();
	}

	core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_irq(_state: &State) -> *mut usize {
	if let Some(irqid) = GicV3::get_and_acknowledge_interrupt(InterruptGroup::Group1) {
		let vector: u8 = u32::from(irqid).try_into().unwrap();

		debug!("Receive interrupt {vector}");
		increment_irq_counter(vector);

		if let Some(handlers) = INTERRUPT_HANDLERS.get()
			&& let Some(queue) = handlers.get(&vector)
		{
			for handler in queue.iter() {
				handler();
			}
		}
		crate::executor::run();
		core_scheduler().handle_waiting_tasks();

		GicV3::end_interrupt(irqid, InterruptGroup::Group1);

		return core_scheduler().scheduler().unwrap_or_default();
	}

	core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_sync(state: &State) {
	let esr = ESR_EL1.get();
	let ec_raw = ESR_EL1.read(ESR_EL1::EC);
	let ec: ESR_EL1::EC::Value = ESR_EL1.read_as_enum(ESR_EL1::EC).unwrap();
	let iss = ESR_EL1.read(ESR_EL1::ISS);
	let pc = ELR_EL1.get();

	/* data abort from lower or current level */
	if (ec == ESR_EL1::EC::Value::SoftwareStepCurrentEL)
		|| (ec == ESR_EL1::EC::Value::SoftwareStepLowerEL)
	{
		/* check if value in far_el1 is valid */
		if (iss & (1 << 10)) == 0 {
			/* read far_el1 register, which holds the faulting virtual address */
			let far = FAR_EL1.get();

			// add page fault handler

			error!("Current stack pointer {state:p}");
			error!("Unable to handle page fault at {far:#x}");
			error!("Exception return address {:#x}", ELR_EL1.get());
			error!("Thread ID register {:#x}", TPIDR_EL0.get());
			error!("Table Base Register {:#x}", TTBR0_EL1.get());
			error!("Exception Syndrome Register {esr:#x}");

			if let Some(irqid) = GicV3::get_and_acknowledge_interrupt(InterruptGroup::Group1) {
				GicV3::end_interrupt(irqid, InterruptGroup::Group1);
			} else {
				error!("Unable to acknowledge interrupt!");
			}

			scheduler::abort()
		} else {
			error!("Unknown exception");
		}
	} else if ec == ESR_EL1::EC::Value::Brk64 {
		error!("Trap to debugger, PC={pc:#x}");
		loop {
			core::hint::spin_loop();
		}
	} else if ec == ESR_EL1::EC::Value::TrappedFP {
		trace!("Floating point trap");

		// We disabled FPU traps to lazily save the FPU state
		// This synchronous exception is triggered when floating point is used
		// So now save and restore the FPU state
		CPACR_EL1.modify(CPACR_EL1::FPEN::TrapNothing);
		isb(SY);

		// Let the scheduler set up the FPU for the current task
		core_scheduler().fpu_switch();
	} else {
		error!("Unsupported exception class: {ec_raw:#x}, PC={pc:#x}");

		loop {
			core::hint::spin_loop();
		}
	}
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_bad_mode(_state: &State, reason: u32) -> ! {
	error!("Receive unhandled exception: {reason}");

	scheduler::abort()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_error(_state: &State) -> ! {
	error!("Receive error interrupt");

	scheduler::abort()
}

pub fn wakeup_core(core_id: CoreId) {
	debug!("Wakeup core {core_id}");
	let reschedid = IntId::sgi(SGI_RESCHED.into());

	GicV3::send_sgi(
		reschedid,
		SgiTarget::List {
			affinity3: 0,
			affinity2: 0,
			affinity1: 0,
			target_list: 1 << core_id,
		},
		SgiTargetGroup::CurrentGroup1,
	);
}

pub(crate) fn init() {
	info!("Initialize generic interrupt controller");

	let fdt = env::fdt().unwrap();

	let intc_node = fdt.find_node("/intc").unwrap();
	let mut reg_iter = intc_node.reg().unwrap();
	let gicd_reg = reg_iter.next().unwrap();
	let gicr_reg = reg_iter.next().unwrap();
	let gicd_start = PhysAddr::from(gicd_reg.starting_address.addr());
	let gicr_start = PhysAddr::from(gicr_reg.starting_address.addr());
	let gicd_size = u64::try_from(gicd_reg.size.unwrap()).unwrap();
	let gicr_size = u64::try_from(gicr_reg.size.unwrap()).unwrap();

	let num_cpus = fdt.cpus().count();

	let cpu_id: usize = core_id().try_into().unwrap();

	let compatible = intc_node
		.compatible()
		.map(Compatible::first)
		.unwrap_or("unknown");
	let is_gic_v4 = if compatible == "arm,gic-v4" {
		info!("Found GIC v4 with {num_cpus} cpus");
		true
	} else if compatible == "arm,gic-v3" {
		info!("Found GIC v3 with {num_cpus} cpus");
		false
	} else {
		panic!("{compatible} isn't supported")
	};

	info!("Found GIC Distributor interface at {gicd_start:p} (size {gicd_size:#X})");
	info!(
		"Found generic interrupt controller redistributor at {gicr_start:p} (size {gicr_size:#X})"
	);

	let layout = PageLayout::from_size_align(gicd_size.try_into().unwrap(), 0x10000).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let gicd_address = VirtAddr::from(page_range.start());
	debug!("Mapping GIC Distributor interface to virtual address {gicd_address:p}");

	let mut flags = PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	paging::map::<BasePageSize>(
		gicd_address,
		gicd_start,
		(gicd_size / BasePageSize::SIZE).try_into().unwrap(),
		flags,
	);

	let layout = PageLayout::from_size_align(gicr_size.try_into().unwrap(), 0x10000).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let gicr_address = VirtAddr::from(page_range.start());
	debug!("Mapping generic interrupt controller to virtual address {gicr_address:p}");
	paging::map::<BasePageSize>(
		gicr_address,
		gicr_start,
		(gicr_size / BasePageSize::SIZE).try_into().unwrap(),
		flags,
	);

	let mut gic = unsafe {
		GicV3::new(
			gicd_address.as_mut_ptr(),
			gicr_address.as_mut_ptr(),
			num_cpus,
			is_gic_v4,
		)
	};
	gic.setup(cpu_id);
	GicV3::set_priority_mask(0xff);

	if let Some(timer_node) = fdt.find_compatible(&["arm,armv8-timer", "arm,armv7-timer"]) {
		let irq_slice = timer_node.property("interrupts").unwrap().value;

		/* Secure Phys IRQ */
		let (_irqtype, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		let (_irq, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		let (_irqflags, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		/* Non-secure Phys IRQ */
		let (irqtype, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		let (irq, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		let (irqflags, _irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		let irqtype = u32::from_be_bytes(irqtype.try_into().unwrap());
		let irq = u32::from_be_bytes(irq.try_into().unwrap());
		let irqflags = u32::from_be_bytes(irqflags.try_into().unwrap());
		unsafe {
			TIMER_INTERRUPT = irq;
		}

		debug!("Timer interrupt: {irq}, type {irqtype}, flags {irqflags}");

		IRQ_NAMES
			.lock()
			.insert(u8::try_from(irq).unwrap() + PPI_START, "Timer");

		// enable timer interrupt
		let timer_irqid = if irqtype == 1 {
			IntId::ppi(irq)
		} else if irqtype == 0 {
			IntId::spi(irq)
		} else {
			panic!("Invalid interrupt type");
		};
		gic.set_interrupt_priority(timer_irqid, Some(cpu_id), 0x00);
		if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
			gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Level);
		} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
			gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Edge);
		} else {
			panic!("Invalid interrupt level!");
		}
		gic.enable_interrupt(timer_irqid, Some(cpu_id), true);
	}

	if let Some(uart_node) = fdt.find_compatible(&["arm,pl011"]) {
		let irq_slice = uart_node.property("interrupts").unwrap().value;
		let (irqtype, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		let (irq, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
		let (irqflags, _) = irq_slice.split_at(core::mem::size_of::<u32>());
		let irqtype = u32::from_be_bytes(irqtype.try_into().unwrap());
		let irq = u32::from_be_bytes(irq.try_into().unwrap());
		let irqflags = u32::from_be_bytes(irqflags.try_into().unwrap());

		unsafe {
			UART_INTERRUPT = irq;
		}

		debug!("UART interrupt: {irq}, type {irqtype}, flags {irqflags}");

		IRQ_NAMES
			.lock()
			.insert(u8::try_from(irq).unwrap() + SPI_START, "UART");

		// enable uart interrupt
		let uart_irqid = if irqtype == 1 {
			IntId::ppi(irq)
		} else if irqtype == 0 {
			IntId::spi(irq)
		} else {
			panic!("Invalid interrupt type");
		};
		gic.set_interrupt_priority(uart_irqid, Some(cpu_id), 0x00);
		if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
			gic.set_trigger(uart_irqid, Some(cpu_id), Trigger::Level);
		} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
			gic.set_trigger(uart_irqid, Some(cpu_id), Trigger::Edge);
		} else {
			panic!("Invalid interrupt level!");
		}
		gic.enable_interrupt(uart_irqid, Some(cpu_id), true);
	}

	let reschedid = IntId::sgi(SGI_RESCHED.into());
	gic.set_interrupt_priority(reschedid, Some(cpu_id), 0x01);
	gic.enable_interrupt(reschedid, Some(cpu_id), true);
	IRQ_NAMES.lock().insert(SGI_RESCHED, "Reschedule");

	*GIC.lock() = Some(gic);
}

// marks the given CPU core as awake
pub fn init_cpu() {
	let cpu_id: usize = core_id().try_into().unwrap();

	if let Some(ref mut gic) = *GIC.lock() {
		debug!("Mark cpu {cpu_id} as awake");

		gic.setup(cpu_id);
		GicV3::set_priority_mask(0xff);

		let fdt = env::fdt().unwrap();

		if let Some(timer_node) = fdt.find_compatible(&["arm,armv8-timer", "arm,armv7-timer"]) {
			let irq_slice = timer_node.property("interrupts").unwrap().value;
			/* Secure Phys IRQ */
			let (_irqtype, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
			let (_irq, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
			let (_irqflags, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
			/* Non-secure Phys IRQ */
			let (irqtype, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
			let (irq, irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
			let (irqflags, _irq_slice) = irq_slice.split_at(core::mem::size_of::<u32>());
			let irqtype = u32::from_be_bytes(irqtype.try_into().unwrap());
			let irq = u32::from_be_bytes(irq.try_into().unwrap());
			let irqflags = u32::from_be_bytes(irqflags.try_into().unwrap());

			// enable timer interrupt
			let timer_irqid = if irqtype == 1 {
				IntId::ppi(irq)
			} else if irqtype == 0 {
				IntId::spi(irq)
			} else {
				panic!("Invalid interrupt type");
			};
			gic.set_interrupt_priority(timer_irqid, Some(cpu_id), 0x00);
			if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
				gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Level);
			} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
				gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Edge);
			} else {
				panic!("Invalid interrupt level!");
			}
			gic.enable_interrupt(timer_irqid, Some(cpu_id), true);
		}

		let reschedid = IntId::sgi(SGI_RESCHED.into());
		gic.set_interrupt_priority(reschedid, Some(cpu_id), 0x01);
		gic.enable_interrupt(reschedid, Some(cpu_id), true);
	}
}

pub(crate) static IRQ_COUNTERS: InterruptSpinMutex<BTreeMap<CoreId, &IrqStatistics>> =
	InterruptSpinMutex::new(BTreeMap::new());

pub(crate) struct IrqStatistics {
	pub counters: [AtomicU64; 256],
}

impl IrqStatistics {
	pub const fn new() -> Self {
		#[allow(clippy::declare_interior_mutable_const)]
		const NEW_COUNTER: AtomicU64 = AtomicU64::new(0);
		IrqStatistics {
			counters: [NEW_COUNTER; 256],
		}
	}

	pub fn inc(&self, pos: u8) {
		self.counters[usize::from(pos)].fetch_add(1, Ordering::Relaxed);
	}
}

pub(crate) fn print_statistics() {
	info!("Number of interrupts");
	for (core_id, irg_statistics) in IRQ_COUNTERS.lock().iter() {
		for (i, counter) in irg_statistics.counters.iter().enumerate() {
			let counter = counter.load(Ordering::Relaxed);
			if counter > 0 {
				match get_irq_name(i.try_into().unwrap()) {
					Some(name) => {
						info!("[{core_id}][{name}]: {counter}");
					}
					_ => {
						info!("[{core_id}][{i}]: {counter}");
					}
				}
			}
		}
	}
}

#[path = "../../../kernel/interrupts.rs"]
mod interrupts_common;
pub(crate) use interrupts_common::*;
