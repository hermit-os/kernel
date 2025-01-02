use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::arch::asm;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

use aarch64::regs::*;
use ahash::RandomState;
use arm_gic::gicv3::{GicV3, IntId, Trigger};
use hashbrown::HashMap;
use hermit_dtb::Dtb;
use hermit_sync::{InterruptSpinMutex, InterruptTicketMutex, OnceCell, SpinMutex};
use memory_addresses::arch::aarch64::PhysAddr;

use crate::arch::aarch64::kernel::core_local::increment_irq_counter;
use crate::arch::aarch64::kernel::scheduler::State;
use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::arch::aarch64::mm::virtualmem;
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_interrupt_handlers;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_interrupt_handlers;
use crate::drivers::{InterruptHandlerQueue, InterruptLine};
use crate::scheduler::{self, CoreId};
use crate::{core_scheduler, env};

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
pub(crate) static GIC: SpinMutex<Option<GicV3>> = SpinMutex::new(None);

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

pub(crate) fn install_handlers() {
	let mut handlers: HashMap<InterruptLine, InterruptHandlerQueue, RandomState> =
		HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0));

	fn timer_handler() {
		debug!("Handle timer interrupt");

		// disable timer
		unsafe {
			asm!(
				"msr cntp_cval_el0, xzr",
				"msr cntp_ctl_el0, xzr",
				options(nostack, nomem),
			);
		}
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

		debug!("Receive fiq {}", vector);
		increment_irq_counter(vector);

		if let Some(handlers) = INTERRUPT_HANDLERS.get() {
			if let Some(queue) = handlers.get(&vector) {
				for handler in queue.iter() {
					handler();
				}
			}
		}
		crate::executor::run();
		core_scheduler().handle_waiting_tasks();

		GicV3::end_interrupt(irqid);

		return core_scheduler()
			.scheduler()
			.unwrap_or(core::ptr::null_mut());
	}

	core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_irq(_state: &State) -> *mut usize {
	if let Some(irqid) = GicV3::get_and_acknowledge_interrupt() {
		let vector: u8 = u32::from(irqid).try_into().unwrap();

		debug!("Receive interrupt {}", vector);
		increment_irq_counter(vector);

		if let Some(handlers) = INTERRUPT_HANDLERS.get() {
			if let Some(queue) = handlers.get(&vector) {
				for handler in queue.iter() {
					handler();
				}
			}
		}
		crate::executor::run();
		core_scheduler().handle_waiting_tasks();

		GicV3::end_interrupt(irqid);

		return core_scheduler()
			.scheduler()
			.unwrap_or(core::ptr::null_mut());
	}

	core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_sync(state: &State) {
	let irqid = GicV3::get_and_acknowledge_interrupt().unwrap();
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
			error!("Unable to handle page fault at {:#x}", far);
			error!("Exception return address {:#x}", ELR_EL1.get());
			error!("Thread ID register {:#x}", TPIDR_EL0.get());
			error!("Table Base Register {:#x}", TTBR0_EL1.get());
			error!("Exception Syndrome Register {:#x}", esr);

			GicV3::end_interrupt(irqid);
			scheduler::abort()
		} else {
			error!("Unknown exception");
		}
	} else if ec == 0x3c {
		error!("Trap to debugger, PC={:#x}", pc);
	} else {
		error!("Unsupported exception class: {:#x}, PC={:#x}", ec, pc);
	}
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_bad_mode(_state: &State, reason: u32) -> ! {
	error!("Receive unhandled exception: {}", reason);

	scheduler::abort()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_error(_state: &State) -> ! {
	error!("Receive error interrupt");

	scheduler::abort()
}

pub fn wakeup_core(_core_to_wakeup: CoreId) {
	todo!("wakeup_core stub");
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
	let gicc_start = PhysAddr::new(u64::from_be_bytes(slice.try_into().unwrap()));
	let (slice, _residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
	let gicc_size = u64::from_be_bytes(slice.try_into().unwrap());

	info!(
		"Found GIC Distributor interface at {:p} (size {:#X})",
		gicd_start, gicd_size
	);
	info!(
		"Found generic interrupt controller at {:p} (size {:#X})",
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
			if core::str::from_utf8(compatible).unwrap().contains("timer") {
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

				debug!(
					"Timer interrupt: {}, type {}, flags {}",
					irq, irqtype, irqflags
				);

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
				gic.set_interrupt_priority(timer_irqid, 0x00);
				if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
					gic.set_trigger(timer_irqid, Trigger::Level);
				} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
					gic.set_trigger(timer_irqid, Trigger::Edge);
				} else {
					panic!("Invalid interrupt level!");
				}
				gic.enable_interrupt(timer_irqid, true);
			}
		}
	}

	let reschedid = IntId::sgi(SGI_RESCHED.into());
	gic.set_interrupt_priority(reschedid, 0x00);
	gic.enable_interrupt(reschedid, true);
	IRQ_NAMES.lock().insert(SGI_RESCHED, "Reschedule");

	*GIC.lock() = Some(gic);
}

static IRQ_NAMES: InterruptTicketMutex<HashMap<u8, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

#[allow(dead_code)]
pub(crate) fn add_irq_name(irq_number: u8, name: &'static str) {
	debug!("Register name \"{}\"  for interrupt {}", name, irq_number);
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
