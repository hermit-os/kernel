use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::arch::asm;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

use aarch64::regs::*;
use ahash::RandomState;
use arm_gic::gicv3::{GicV3, SgiTarget};
use arm_gic::{IntId, Trigger};
use hashbrown::HashMap;
use hermit_dtb::Dtb;
use hermit_sync::{InterruptSpinMutex, InterruptTicketMutex, OnceCell, SpinMutex};
use memory_addresses::arch::aarch64::PhysAddr;

use crate::arch::aarch64::kernel::core_local::increment_irq_counter;
use crate::arch::aarch64::kernel::scheduler::State;
use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_interrupt_handlers;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_interrupt_handlers;
use crate::drivers::{InterruptHandlerQueue, InterruptLine};
use crate::mm::virtualmem;
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
/// Possible interrupt handlers
static INTERRUPT_HANDLERS: OnceCell<HashMap<u8, InterruptHandlerQueue, RandomState>> =
	OnceCell::new();
/// Driver for the Arm Generic Interrupt Controller version 3 (or 4).
pub(crate) static GIC: SpinMutex<Option<GicV3<'_>>> = SpinMutex::new(None);

/// Enable all interrupts
#[inline]
pub fn enable() {
	unsafe {
		asm!(
			"dmb ish",
			"msr daifclr, {mask}",
			"dmb ish",
			mask = const 0b111,
			options(nostack),
		);
	}
}

/// Enable all interrupts and wait for the next interrupt (wfi instruction)
#[inline]
pub fn enable_and_wait() {
	unsafe {
		asm!(
			"dmb ish",
			"msr daifclr, {mask}; wfi",
			"dmb ish",
			mask = const 0b111,
			options(nostack),
		);
	}
}

/// Disable all interrupts
#[inline]
pub fn disable() {
	unsafe {
		asm!(
			"dmb ish",
			"msr daifset, {mask}",
			"dmb ish",
			mask = const 0b111,
			options(nostack),
		);
	}
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
	}

	INTERRUPT_HANDLERS.set(handlers).unwrap();
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_fiq(_state: &State) -> *mut usize {
	if let Some(irqid) = GicV3::get_and_acknowledge_interrupt() {
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

		GicV3::end_interrupt(irqid);

		return core_scheduler().scheduler().unwrap_or_default();
	}

	core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_irq(_state: &State) -> *mut usize {
	if let Some(irqid) = GicV3::get_and_acknowledge_interrupt() {
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

		GicV3::end_interrupt(irqid);

		return core_scheduler().scheduler().unwrap_or_default();
	}

	core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_sync(state: &State) {
	let esr = ESR_EL1.get();
	let ec = esr >> 26;
	let iss = esr & 0x00ff_ffff;
	let pc = ELR_EL1.get();

	/* data abort from lower or current level */
	if (ec == 0b10_0100) || (ec == 0b10_0101) {
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

			if let Some(irqid) = GicV3::get_and_acknowledge_interrupt() {
				GicV3::end_interrupt(irqid);
			} else {
				error!("Unable to acknowledge interrupt!");
			}

			scheduler::abort()
		} else {
			error!("Unknown exception");
		}
	} else if ec == 0x3c {
		error!("Trap to debugger, PC={pc:#x}");
		loop {
			core::hint::spin_loop();
		}
	} else {
		error!("Unsupported exception class: {ec:#x}, PC={pc:#x}");
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
	);
}

pub(crate) fn init() {
	info!("Initialize generic interrupt controller");

	let dtb = unsafe {
		Dtb::from_raw(ptr::with_exposed_provenance(
			env::boot_info().hardware_info.device_tree.unwrap().get() as usize,
		))
		.expect(".dtb file has invalid header")
	};

	let reg = dtb.get_property("/intc", "reg").unwrap();
	let (slice, residual_slice) = reg.split_at(core::mem::size_of::<u64>());
	let gicd_start = PhysAddr::new(u64::from_be_bytes(slice.try_into().unwrap()));
	let (slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
	let gicd_size = u64::from_be_bytes(slice.try_into().unwrap());
	let (slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
	let gicr_start = PhysAddr::new(u64::from_be_bytes(slice.try_into().unwrap()));
	let (slice, _residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
	let gicr_size = u64::from_be_bytes(slice.try_into().unwrap());

	let num_cpus = dtb
		.enum_subnodes("/cpus")
		.filter(|name| name.contains("cpu@"))
		.count();
	let cpu_id: usize = core_id().try_into().unwrap();

	let compatible = core::str::from_utf8(
		dtb.get_property("/intc", "compatible")
			.unwrap_or(b"unknown"),
	)
	.unwrap()
	.replace('\0', "");
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

	let gicr_address =
		virtualmem::allocate_aligned(gicr_size.try_into().unwrap(), 0x10000).unwrap();
	debug!("Mapping generic interrupt controller to virtual address {gicr_address:p}",);
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

	for node in dtb.enum_subnodes("/") {
		let parts: Vec<_> = node.split('@').collect();

		if let Some(compatible) = dtb.get_property(parts.first().unwrap(), "compatible")
			&& core::str::from_utf8(compatible).unwrap().contains("timer")
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

		let dtb = unsafe {
			Dtb::from_raw(ptr::with_exposed_provenance(
				env::boot_info().hardware_info.device_tree.unwrap().get() as usize,
			))
			.expect(".dtb file has invalid header")
		};

		for node in dtb.enum_subnodes("/") {
			let parts: Vec<_> = node.split('@').collect();

			if let Some(compatible) = dtb.get_property(parts.first().unwrap(), "compatible")
				&& core::str::from_utf8(compatible).unwrap().contains("timer")
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
		}

		let reschedid = IntId::sgi(SGI_RESCHED.into());
		gic.set_interrupt_priority(reschedid, Some(cpu_id), 0x01);
		gic.enable_interrupt(reschedid, Some(cpu_id), true);
	}
}

static IRQ_NAMES: InterruptTicketMutex<HashMap<u8, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

#[allow(dead_code)]
pub(crate) fn add_irq_name(irq_number: u8, name: &'static str) {
	debug!("Register name \"{name}\"  for interrupt {irq_number}");
	IRQ_NAMES.lock().insert(SPI_START + irq_number, name);
}

fn get_irq_name(irq_number: u8) -> Option<&'static str> {
	IRQ_NAMES.lock().get(&irq_number).copied()
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
