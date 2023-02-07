use alloc::collections::BTreeMap;
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use ahash::RandomState;
use hashbrown::HashMap;
use hermit_sync::{InterruptSpinMutex, InterruptTicketMutex};
pub use x86_64::instructions::interrupts::{disable, enable, enable_and_hlt as enable_and_wait};
use x86_64::registers::control::Cr2;
use x86_64::set_general_handler;
pub use x86_64::structures::idt::InterruptStackFrame as ExceptionStackFrame;
use x86_64::structures::idt::{InterruptDescriptorTable, PageFaultErrorCode};

use crate::arch::x86_64::kernel::core_local::{core_scheduler, increment_irq_counter};
use crate::arch::x86_64::kernel::{apic, processor};
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::scheduler::{self, CoreId};

pub const IST_ENTRIES: usize = 3;
pub const IST_SIZE: usize = BasePageSize::SIZE as usize;

pub static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn load_idt() {
	unsafe {
		IDT.load_unsafe();
	}
}

pub fn install() {
	let idt = unsafe { &mut IDT };

	set_general_handler!(idt, abort, 0..32);
	set_general_handler!(idt, unhandle, 32..64);
	set_general_handler!(idt, unknown, 64..);

	unsafe {
		idt.double_fault
			.set_handler_fn(double_fault_exception)
			.set_stack_index(0);
		idt.non_maskable_interrupt
			.set_handler_fn(nmi_exception)
			.set_stack_index(1);
		idt.machine_check
			.set_handler_fn(machine_check_exception)
			.set_stack_index(2);
	}
	idt.device_not_available
		.set_handler_fn(device_not_available_exception);
	idt.page_fault.set_handler_fn(page_fault_handler);
}

#[no_mangle]
pub extern "C" fn irq_install_handler(irq_number: u32, handler: usize) {
	debug!("Install handler for interrupt {}", irq_number);

	let idt = unsafe { &mut IDT };
	unsafe {
		idt[(32 + irq_number) as usize].set_handler_addr(x86_64::VirtAddr::new(handler as u64));
	}
}

fn abort(stack_frame: ExceptionStackFrame, index: u8, error_code: Option<u64>) {
	error!("Exception {index}");
	error!("Error code: {error_code:?}");
	error!("Stack frame: {stack_frame:#?}");
	scheduler::abort();
}

fn unhandle(_stack_frame: ExceptionStackFrame, index: u8, _error_code: Option<u64>) {
	warn!("received unhandled irq {index}");
	apic::eoi();
	increment_irq_counter(index.into());
}

fn unknown(_stack_frame: ExceptionStackFrame, index: u8, _error_code: Option<u64>) {
	warn!("unknown interrupt {index}");
	apic::eoi();
}

extern "x86-interrupt" fn nmi_exception(stack_frame: ExceptionStackFrame) {
	error!("Non-Maskable Interrupt (NMI) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn device_not_available_exception(_stack_frame: ExceptionStackFrame) {
	// We set the CR0_TASK_SWITCHED flag every time we switch to a task.
	// This causes the "Device Not Available" Exception (int #7) to be thrown as soon as we use the FPU for the first time.

	increment_irq_counter(7);

	// Clear CR0_TASK_SWITCHED so this doesn't happen again before the next switch.
	unsafe {
		asm!("clts", options(nomem, nostack));
	}

	// Let the scheduler set up the FPU for the current task.
	core_scheduler().fpu_switch();
}

extern "x86-interrupt" fn double_fault_exception(
	stack_frame: ExceptionStackFrame,
	error_code: u64,
) -> ! {
	error!(
		"Double Fault (#DF) Exception: {:#?}, error {:#X}",
		stack_frame, error_code
	);
	scheduler::abort()
}

pub extern "x86-interrupt" fn page_fault_handler(
	stack_frame: ExceptionStackFrame,
	error_code: PageFaultErrorCode,
) {
	error!("Page fault (#PF)!");
	error!("page_fault_linear_address = {:p}", Cr2::read());
	error!("error_code = {error_code:?}");
	error!("fs = {:#X}", processor::readfs());
	error!("gs = {:#X}", processor::readgs());
	error!("stack_frame = {stack_frame:#?}");
	scheduler::abort();
}

extern "x86-interrupt" fn machine_check_exception(stack_frame: ExceptionStackFrame) -> ! {
	error!("Machine Check (#MC) Exception: {:#?}", stack_frame);
	scheduler::abort()
}

static IRQ_NAMES: InterruptTicketMutex<HashMap<u32, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

pub fn add_irq_name(irq_number: u32, name: &'static str) {
	debug!("Register name \"{}\"  for interrupt {}", name, irq_number);
	IRQ_NAMES.lock().insert(32 + irq_number, name);
}

fn get_irq_name(irq_number: u32) -> Option<&'static str> {
	IRQ_NAMES.lock().get(&irq_number).copied()
}

pub static IRQ_COUNTERS: InterruptSpinMutex<BTreeMap<CoreId, &IrqStatistics>> =
	InterruptSpinMutex::new(BTreeMap::new());

pub struct IrqStatistics {
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

	pub fn inc(&self, pos: usize) {
		self.counters[pos].fetch_add(1, Ordering::Relaxed);
	}
}

pub fn print_statistics() {
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
