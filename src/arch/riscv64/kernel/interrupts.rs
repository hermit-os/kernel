use alloc::vec::Vec;
use core::mem::offset_of;
use core::num::NonZeroU16;
use core::ptr::NonNull;

use ahash::RandomState;
use bit_field::BitField;
use hashbrown::HashMap;
use hermit_sync::{InterruptTicketMutex, OnceCell, SpinMutex};
use riscv::asm::wfi;
use riscv::interrupt::{Exception, Interrupt, Trap};
use riscv::register::{scause, sie, sip, sstatus, stval};
use trapframe::TrapFrame;
use volatile::access::{NoAccess, ReadOnly};
use volatile::{VolatileFieldAccess, VolatilePtr, VolatileRef};

use crate::drivers::InterruptHandlerMap;
use crate::scheduler;

const NUMBER_OF_SOURCES: usize = 1024;
const NUMBER_OF_CONTEXTS: usize = 15871;

const INTERRUPT_PENDING_BITS_OFFSET: usize = 0x00_1000;
const INTERRUPT_ENABLE_BITS_OFFSET: usize = 0x00_2000;
const CONTEXT_BASED_REGISTERS: usize = 0x20_0000;

type SourceBitArray = [u32; NUMBER_OF_SOURCES / (u32::BITS as usize)];

#[repr(C, align(4096))]
#[derive(VolatileFieldAccess)]
struct ContextBasedRegisters {
	priority_threshold: u32,
	claim_or_complete: u32,
}

#[repr(C)]
#[derive(VolatileFieldAccess)]
struct Plic {
	#[access(NoAccess)]
	_reserved0: u32,
	interrupt_priorities: [u32; NUMBER_OF_SOURCES - 1],
	#[access(ReadOnly)]
	interrupt_pending_bits: SourceBitArray,
	#[access(NoAccess)]
	_reserved3: [u32; (INTERRUPT_ENABLE_BITS_OFFSET - 0x00_1080) / size_of::<u32>()],
	interrupt_enable_bits: [SourceBitArray; NUMBER_OF_CONTEXTS],
	#[access(NoAccess)]
	_reserved2: [u32; (CONTEXT_BASED_REGISTERS - 0x1f_2000) / size_of::<u32>()],
	context_based_registers: [ContextBasedRegisters; NUMBER_OF_CONTEXTS],
}

const _: () = if offset_of!(Plic, interrupt_pending_bits) != INTERRUPT_PENDING_BITS_OFFSET
	|| offset_of!(Plic, interrupt_enable_bits) != INTERRUPT_ENABLE_BITS_OFFSET
	|| offset_of!(Plic, context_based_registers) != CONTEXT_BASED_REGISTERS
{
	panic!();
};

impl Plic {
	fn interrupt_priority<'a>(
		ptr: VolatilePtr<'a, Self>,
		source: NonZeroU16,
	) -> VolatilePtr<'a, u32> {
		unsafe {
			ptr.interrupt_priorities().map(|slice| {
				slice
					.cast()
					.offset(isize::try_from(source.get()).unwrap() - 1)
			})
		}
	}

	fn context_based_register<'a>(
		ptr: VolatilePtr<'a, Self>,
		context: u16,
	) -> VolatilePtr<'a, ContextBasedRegisters> {
		unsafe {
			ptr.context_based_registers()
				.map(|slice| slice.cast().offset(isize::try_from(context).unwrap()))
		}
	}

	fn set_enable_bit(ptr: VolatilePtr<'_, Self>, context: u16, source: NonZeroU16, value: bool) {
		let source = usize::from(source.get());
		unsafe {
			ptr.interrupt_enable_bits()
				.map(|slice| {
					slice
						.cast::<SourceBitArray>()
						.offset(isize::try_from(context).unwrap())
				})
				.map(|context_slice| {
					context_slice
						.cast::<u32>()
						.offset((source / 32).try_into().unwrap())
				})
				.update(|mut word| {
					word.set_bit(source % 32, value);
					word
				});
		};
	}
}

static PLIC: SpinMutex<core::cell::OnceCell<VolatileRef<'static, Plic>>> =
	SpinMutex::new(core::cell::OnceCell::new());

/// PLIC context for new interrupt handlers
static PLIC_CONTEXT: OnceCell<u16> = OnceCell::new();

/// PLIC context for new interrupt handlers
static CURRENT_INTERRUPTS: SpinMutex<Vec<u32>> = SpinMutex::new(Vec::new());

static INTERRUPT_HANDLERS: OnceCell<InterruptHandlerMap> = OnceCell::new();

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
pub(crate) fn init_plic(base: *const u8, context: u16) {
	PLIC.lock()
		.set(unsafe { VolatileRef::new(NonNull::new(base.cast::<Plic>().cast_mut()).unwrap()) })
		.unwrap();
	PLIC_CONTEXT.set(context).unwrap();
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
pub(crate) fn install_handlers(handlers: InterruptHandlerMap) {
	for irq_number in handlers.keys() {
		let mut plic_guard = PLIC.lock();
		let plic_ptr = plic_guard.get_mut().unwrap().as_mut_ptr();
		let context = *PLIC_CONTEXT.get().unwrap();
		let source = NonZeroU16::new(u16::from(*irq_number)).unwrap();

		// Set priority to 7 (highest on FU740)
		Plic::interrupt_priority(plic_ptr, source).write(1);
		// Set Threshold to 0 (lowest)
		Plic::context_based_register(plic_ptr, context)
			.priority_threshold()
			.write(0);
		// Enable irq for context
		Plic::set_enable_bit(plic_ptr, context, source, true);
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
	let mut plic_guard = PLIC.lock();
	let plic_ptr = plic_guard.get_mut().unwrap().as_mut_ptr();
	let context = *PLIC_CONTEXT.get().unwrap();
	let claim_ptr = Plic::context_based_register(plic_ptr, context).claim_or_complete();
	let irq = claim_ptr.read();

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
		claim_ptr.write(irq);

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
