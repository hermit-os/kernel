use alloc::collections::{BTreeMap, VecDeque};
use core::arch::asm;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU64, Ordering};

use aarch64_cpu::asm::barrier::{ISH, SY, dmb, isb};
use aarch64_cpu::registers::*;
use ahash::RandomState;
use arm_gic::gicv3::{GicCpuInterface, GicV3, SgiTarget, SgiTargetGroup};
use arm_gic::{IntId, InterruptGroup, Trigger, UniqueMmioPointer};
use fdt::standard_nodes::Compatible;
use free_list::PageLayout;
use hashbrown::HashMap;
use hermit_sync::{InterruptSpinMutex, InterruptTicketMutex, OnceCell, SpinMutex};
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::aarch64::kernel::core_local::{core_id, core_scheduler, increment_irq_counter};
use crate::arch::aarch64::kernel::scheduler::State;
use crate::arch::aarch64::kernel::serial::handle_uart_interrupt;
use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::drivers::InterruptHandlerMap;
use crate::env;
use crate::mm::{PageAlloc, PageRangeAllocator};
use crate::scheduler::{self, CoreId, timer_interrupts};

/// The ID of the first Private Peripheral Interrupt.
const PPI_START: u8 = 16;
/// The ID of the first Shared Peripheral Interrupt.
#[allow(dead_code)]
const SPI_START: u8 = 32;
/// Software-generated interrupt for rescheduling
pub(crate) const SGI_RESCHED: u8 = 1;
/// Synthetic IRQ slot used for page-fault accounting. The number does not
/// correspond to any GIC interrupt — it is purely a bookkeeping ID for
/// `IrqStatistics` so the page-fault count shows up in `print_statistics`
/// alongside the real interrupts.  Picked at 14 to mirror the x86_64
/// CPU exception vector for #PF, which keeps the two architectures
/// consistent in the diagnostic output.
#[cfg(feature = "common-os")]
pub(crate) const PAGE_FAULT_IRQ: u8 = 14;

/// Number of the timer interrupt
static mut TIMER_INTERRUPT: u32 = 0;
/// Number of the UART interrupt
static mut UART_INTERRUPT: u32 = 0;
/// Possible interrupt handlers
static INTERRUPT_HANDLERS: OnceCell<InterruptHandlerMap> = OnceCell::new();
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

pub(crate) fn install_handlers(old_handlers: InterruptHandlerMap) {
	let mut handlers: InterruptHandlerMap =
		HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0));
	fn timer_handler() {
		debug!("Handle timer interrupt");
		timer_interrupts::clear_active_and_set_next();
	}

	for (key, value) in old_handlers.into_iter() {
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
	let Some(irqid) = GicCpuInterface::get_and_acknowledge_interrupt(InterruptGroup::Group1) else {
		return ptr::null_mut();
	};

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

	GicCpuInterface::end_interrupt(irqid, InterruptGroup::Group1);

	core_scheduler().scheduler().unwrap_or_default()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_irq(_state: &State) -> *mut usize {
	let Some(irqid) = GicCpuInterface::get_and_acknowledge_interrupt(InterruptGroup::Group1) else {
		return ptr::null_mut();
	};

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

	GicCpuInterface::end_interrupt(irqid, InterruptGroup::Group1);

	core_scheduler().scheduler().unwrap_or_default()
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn do_sync(state: &mut State) {
	let esr = ESR_EL1.get();
	let ec_raw = ESR_EL1.read(ESR_EL1::EC);
	let ec: ESR_EL1::EC::Value = ESR_EL1.read_as_enum(ESR_EL1::EC).unwrap();
	let iss = ESR_EL1.read(ESR_EL1::ISS);
	let pc = ELR_EL1.get();

	// SVC64 — user-space syscall (ESR_EL1.EC = 0x15). Dispatched here
	// before any of the other branches so we can return early without
	// touching debug/breakpoint/FPU paths.
	#[cfg(feature = "common-os")]
	if ec_raw == 0x15 {
		dispatch_svc64(state);
		return;
	}

	// User-mode instruction-fetch fault at PC=0: the entry wrapper of a
	// freshly spawned user thread (`std::sys::thread::hermit::Thread::
	// new_with_coreid::thread_start`) returns with `ret`, popping LR
	// from the user stack. The trap frame we crafted in
	// `Task::create_user_stack_frame` zeroed every register, so LR=0
	// and the implicit branch lands at PC 0 — there is no code there.
	// Mirror the x86_64 page-fault handler and treat this as a clean
	// thread exit instead of crashing the whole process.
	#[cfg(feature = "common-os")]
	if ec == ESR_EL1::EC::Value::InstrAbortLowerEL && ELR_EL1.get() == 0 {
		use crate::scheduler::PerCoreSchedulerExt;
		debug!(
			"User thread {} returned from entry; exiting cleanly.",
			core_scheduler().get_current_task_id()
		);
		core_scheduler().exit(0);
	}

	/* Data Abort from current or lower EL — the primary path is a COW
	 * write fault from EL0 (EC=0x25). EC=0x24 covers a kernel write to a
	 * COW-marked page, which can happen e.g. when the kernel writes
	 * argv/envp into the freshly-mapped user page during the loader path.
	 */
	if ec == ESR_EL1::EC::Value::DataAbortCurrentEL
		|| ec == ESR_EL1::EC::Value::DataAbortLowerEL
	{
		#[cfg(feature = "common-os")]
		increment_irq_counter(PAGE_FAULT_IRQ);

		let far = FAR_EL1.get();
		// ESR_EL1.ISS layout for Data Abort (ARM ARM D24.2.45):
		//   bits  5:0 = DFSC (Data Fault Status Code)
		//   bit     6 = WnR (1 = write, 0 = read)
		// Permission fault DFSC values are 0b001100..0b001111 (level 0..3).
		let dfsc = iss & 0b11_1111;
		let is_write = (iss & (1 << 6)) != 0;
		#[cfg(all(feature = "common-os", feature = "fork"))]
		let is_permission_fault = (0b00_1100..=0b00_1111).contains(&dfsc);

		#[cfg(all(feature = "common-os", feature = "fork"))]
		if is_write
			&& is_permission_fault
			&& crate::arch::aarch64::mm::paging::do_cow_fault(VirtAddr::new(far))
		{
			// Faulting instruction is retried on `eret` from the trap.
			return;
		}

		let kind = dfsc_kind(dfsc);
		let access = if is_write { "write" } else { "read" };
		error!("Current stack pointer {state:p}");
		error!(
			"Unhandled data abort: {kind} on {access} of {far:#x} (DFSC={dfsc:#x})"
		);
		error!("Exception return address {:#x}", ELR_EL1.get());
		error!("Thread ID register {:#x}", TPIDR_EL0.get());
		error!("Table Base Register {:#x}", TTBR0_EL1.get());
		error!("Exception Syndrome Register {esr:#x}");

		if let Some(irqid) =
			GicCpuInterface::get_and_acknowledge_interrupt(InterruptGroup::Group1)
		{
			GicCpuInterface::end_interrupt(irqid, InterruptGroup::Group1);
		} else {
			error!("Unable to acknowledge interrupt!");
		}

		scheduler::abort()
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

/// Convert the 6-bit DFSC (Data Fault Status Code) of `ESR_EL1.ISS` into
/// a short human-readable label for diagnostics. Mapping per ARM ARM
/// D24.2.45 (ESR_EL1, Data Abort).
fn dfsc_kind(dfsc: u64) -> &'static str {
	match dfsc {
		0b00_0000..=0b00_0011 => "address size fault",
		0b00_0100..=0b00_0111 => "translation fault",
		0b00_1000..=0b00_1011 => "access flag fault",
		0b00_1100..=0b00_1111 => "permission fault",
		0b01_0000 => "synchronous external abort",
		0b01_0001 => "synchronous tag check fail",
		0b01_0100..=0b01_0111 => "external abort on translation table walk",
		0b01_1000 => "synchronous parity/ECC error",
		0b01_1100..=0b01_1111 => "parity/ECC error on translation table walk",
		0b10_0001 => "alignment fault",
		0b11_0000 => "TLB conflict abort",
		0b11_0001 => "unsupported atomic hardware-update fault",
		_ => "unknown data fault",
	}
}

/// Dispatch an EL0-issued AArch64 syscall (`svc #0`).
///
/// Hermit's user-space follows the Linux-like AArch64 convention:
///   - syscall number in `x8`
///   - up to 6 arguments in `x0`..`x5`
///   - return value in `x0`
///
/// The handler entries in `SYSHANDLER_TABLE` are typed for the SystemV
/// x86_64 ABI but are reachable just as well via the AAPCS64 ABI: in both
/// cases the first six 64-bit args land in the first six argument
/// registers and the return value goes into the first return register.
/// Registers in `state` were saved by `trap_entry` and are restored by
/// `trap_exit` — writing back `state.x0` is what propagates the return
/// value to user-space.
#[cfg(feature = "common-os")]
fn dispatch_svc64(state: &mut State) {
	use crate::errno::Errno;
	use crate::syscalls::table::{NO_SYSCALLS, SYSHANDLER_TABLE, invalid_syscall, sys_invalid};

	let nr = state.x8 as usize;

	if nr >= NO_SYSCALLS {
		error!("Invalid syscall number {nr}");
		state.x0 = (-i32::from(Errno::Nosys)) as u32 as u64;
		return;
	}

	let handler_ptr = SYSHANDLER_TABLE.handler(nr);
	if handler_ptr == sys_invalid as *const usize {
		invalid_syscall(state.x8);
	}

	let f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64 =
		unsafe { core::mem::transmute(handler_ptr) };
	state.x0 = f(state.x0, state.x1, state.x2, state.x3, state.x4, state.x5);
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

	GicCpuInterface::send_sgi(
		reschedid,
		SgiTarget::List {
			affinity3: 0,
			affinity2: 0,
			affinity1: 0,
			target_list: 1 << core_id,
		},
		SgiTargetGroup::CurrentGroup1,
	)
	.unwrap();
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

	let gicd = unsafe { UniqueMmioPointer::new(NonNull::new(gicd_address.as_mut_ptr()).unwrap()) };
	let gicr = NonNull::new(gicr_address.as_mut_ptr()).unwrap();

	let mut gic = unsafe { GicV3::new(gicd, gicr, num_cpus, is_gic_v4) };
	gic.setup(cpu_id);
	GicCpuInterface::set_priority_mask(0xff);

	if let Some(timer_node) = fdt.find_compatible(&["arm,armv8-timer", "arm,armv7-timer"]) {
		let irq_slice = timer_node.property("interrupts").unwrap().value;

		// The "arm,armv8-timer" interrupts property lists four (type, irq,
		// flags) triplets in this exact order:
		//     1. Secure Phys
		//     2. Non-secure Phys
		//     3. Virtual           ← we want this one
		//     4. Hypervisor Phys
		// We program the Virtual Timer (CNTV_*) instead of the Physical
		// Timer because virtualised guests (e.g. macOS HVF) hide the
		// physical timer from EL1, and CNTV_* works identically on bare
		// metal where CNTVOFF_EL2 defaults to 0.

		/* Secure Phys IRQ — skip */
		let (_irqtype, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		let (_irq, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		let (_irqflags, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		/* Non-secure Phys IRQ — skip */
		let (_irqtype, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		let (_irq, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		let (_irqflags, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		/* Virtual Timer IRQ */
		let (irqtype, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		let (irq, irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
		let (irqflags, _irq_slice) = irq_slice.split_at(mem::size_of::<u32>());
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
		gic.set_interrupt_priority(timer_irqid, Some(cpu_id), 0x00)
			.unwrap();
		if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
			gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Level)
				.unwrap();
		} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
			gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Edge)
				.unwrap();
		} else {
			panic!("Invalid interrupt level!");
		}
		gic.enable_interrupt(timer_irqid, Some(cpu_id), true)
			.unwrap();
	}

	if let Some(uart_node) = fdt.find_compatible(&["arm,pl011"]) {
		let irq_slice = uart_node.property("interrupts").unwrap().value;
		let (irqtype, irq_slice) = irq_slice.split_at(size_of::<u32>());
		let (irq, irq_slice) = irq_slice.split_at(size_of::<u32>());
		let (irqflags, _) = irq_slice.split_at(size_of::<u32>());
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
		gic.set_interrupt_priority(uart_irqid, Some(cpu_id), 0x00)
			.unwrap();
		if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
			gic.set_trigger(uart_irqid, Some(cpu_id), Trigger::Level)
				.unwrap();
		} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
			gic.set_trigger(uart_irqid, Some(cpu_id), Trigger::Edge)
				.unwrap();
		} else {
			panic!("Invalid interrupt level!");
		}
		gic.enable_interrupt(uart_irqid, Some(cpu_id), true)
			.unwrap();
	}

	let reschedid = IntId::sgi(SGI_RESCHED.into());
	gic.set_interrupt_priority(reschedid, Some(cpu_id), 0x01)
		.unwrap();
	gic.enable_interrupt(reschedid, Some(cpu_id), true).unwrap();
	IRQ_NAMES.lock().insert(SGI_RESCHED, "Reschedule");
	#[cfg(feature = "common-os")]
	IRQ_NAMES.lock().insert(PAGE_FAULT_IRQ, "Page Fault");

	*GIC.lock() = Some(gic);
}

// marks the given CPU core as awake
pub fn init_cpu() {
	let cpu_id: usize = core_id().try_into().unwrap();

	let mut gic = GIC.lock();
	let Some(gic) = &mut *gic else {
		return;
	};

	debug!("Mark cpu {cpu_id} as awake");

	gic.init_cpu(cpu_id);
	GicCpuInterface::enable_group1(true);
	GicCpuInterface::set_priority_mask(0xff);

	let fdt = env::fdt().unwrap();

	if let Some(timer_node) = fdt.find_compatible(&["arm,armv8-timer", "arm,armv7-timer"]) {
		let irq_slice = timer_node.property("interrupts").unwrap().value;
		/* Secure Phys IRQ */
		let (_irqtype, irq_slice) = irq_slice.split_at(size_of::<u32>());
		let (_irq, irq_slice) = irq_slice.split_at(size_of::<u32>());
		let (_irqflags, irq_slice) = irq_slice.split_at(size_of::<u32>());
		/* Non-secure Phys IRQ */
		let (irqtype, irq_slice) = irq_slice.split_at(size_of::<u32>());
		let (irq, irq_slice) = irq_slice.split_at(size_of::<u32>());
		let (irqflags, _irq_slice) = irq_slice.split_at(size_of::<u32>());
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
		gic.set_interrupt_priority(timer_irqid, Some(cpu_id), 0x00)
			.unwrap();
		if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
			gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Level)
				.unwrap();
		} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
			gic.set_trigger(timer_irqid, Some(cpu_id), Trigger::Edge)
				.unwrap();
		} else {
			panic!("Invalid interrupt level!");
		}
		gic.enable_interrupt(timer_irqid, Some(cpu_id), true)
			.unwrap();
	}

	let reschedid = IntId::sgi(SGI_RESCHED.into());
	gic.set_interrupt_priority(reschedid, Some(cpu_id), 0x01)
		.unwrap();
	gic.enable_interrupt(reschedid, Some(cpu_id), true).unwrap();
}

static IRQ_NAMES: InterruptTicketMutex<HashMap<u8, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

#[allow(dead_code)]
pub(crate) fn add_irq_name(irq_number: u8, name: &'static str) {
	debug!("Register name \"{name}\" for interrupt {irq_number}");
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
