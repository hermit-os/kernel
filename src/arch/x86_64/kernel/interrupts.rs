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
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::arch::x86_64::kernel::core_local::{core_scheduler, increment_irq_counter};
use crate::arch::x86_64::kernel::{apic, processor};
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::scheduler::{self, CoreId};

pub const IST_ENTRIES: usize = 4;
pub const IST_SIZE: usize = 8 * BasePageSize::SIZE as usize;

pub static IDT: InterruptSpinMutex<InterruptDescriptorTable> =
	InterruptSpinMutex::new(InterruptDescriptorTable::new());

pub fn load_idt() {
	// FIXME: This is not sound! For this to be sound, the table must never be
	// modified or destroyed while in use. This is _not_ the case here. Instead, we
	// disable interrupts on the current core when modifying the table and hope for
	// the best in regards to interrupts on other cores.
	unsafe {
		(*IDT.data_ptr()).load_unsafe();
	}
}

pub fn install() {
	let mut idt = IDT.lock();

	set_general_handler!(&mut *idt, abort, 0..32);
	set_general_handler!(&mut *idt, unhandle, 32..64);
	set_general_handler!(&mut *idt, unknown, 64..);

	unsafe {
		for i in 32..256 {
			let addr = idt[i].handler_addr();
			idt[i].set_handler_addr(addr).set_stack_index(0);
		}

		idt.divide_error
			.set_handler_fn(divide_error_exception)
			.set_stack_index(0);
		idt.debug.set_handler_fn(debug_exception).set_stack_index(0);
		idt.breakpoint
			.set_handler_fn(breakpoint_exception)
			.set_stack_index(0);
		idt.overflow
			.set_handler_fn(overflow_exception)
			.set_stack_index(0);
		idt.bound_range_exceeded
			.set_handler_fn(bound_range_exceeded_exception)
			.set_stack_index(0);
		idt.invalid_opcode
			.set_handler_fn(invalid_opcode_exception)
			.set_stack_index(0);
		idt.device_not_available
			.set_handler_fn(device_not_available_exception)
			.set_stack_index(0);
		idt.invalid_tss
			.set_handler_fn(invalid_tss_exception)
			.set_stack_index(0);
		idt.segment_not_present
			.set_handler_fn(segment_not_present_exception)
			.set_stack_index(0);
		idt.stack_segment_fault
			.set_handler_fn(stack_segment_fault_exception)
			.set_stack_index(0);
		idt.general_protection_fault
			.set_handler_fn(general_protection_exception)
			.set_stack_index(0);
		idt.page_fault
			.set_handler_fn(page_fault_handler)
			.set_stack_index(0);
		idt.x87_floating_point
			.set_handler_fn(floating_point_exception)
			.set_stack_index(0);
		idt.alignment_check
			.set_handler_fn(alignment_check_exception)
			.set_stack_index(0);
		idt.simd_floating_point
			.set_handler_fn(simd_floating_point_exception)
			.set_stack_index(0);
		idt.virtualization
			.set_handler_fn(virtualization_exception)
			.set_stack_index(0);
		idt.double_fault
			.set_handler_fn(double_fault_exception)
			.set_stack_index(1);
		idt.non_maskable_interrupt
			.set_handler_fn(nmi_exception)
			.set_stack_index(2);
		idt.machine_check
			.set_handler_fn(machine_check_exception)
			.set_stack_index(3);
		idt.device_not_available
			.set_handler_fn(device_not_available_exception)
			.set_stack_index(0);
	}

	IRQ_NAMES.lock().insert(7, "FPU");
}

#[no_mangle]
pub extern "C" fn irq_install_handler(
	irq_number: u8,
	handler: extern "x86-interrupt" fn(InterruptStackFrame),
) {
	debug!("Install handler for interrupt {}", irq_number);

	let mut idt = IDT.lock();
	unsafe {
		idt[(32 + irq_number) as usize]
			.set_handler_addr(x86_64::VirtAddr::new(
				u64::try_from(handler as usize).unwrap(),
			))
			.set_stack_index(0);
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
	increment_irq_counter(index);
}

fn unknown(_stack_frame: ExceptionStackFrame, index: u8, _error_code: Option<u64>) {
	warn!("unknown interrupt {index}");
	apic::eoi();
}

extern "x86-interrupt" fn divide_error_exception(stack_frame: ExceptionStackFrame) {
	error!("Divide Error (#DE) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn debug_exception(stack_frame: ExceptionStackFrame) {
	error!("Debug (#DB) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn nmi_exception(stack_frame: ExceptionStackFrame) {
	error!("Non-Maskable Interrupt (NMI) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn breakpoint_exception(stack_frame: ExceptionStackFrame) {
	error!("Breakpoint (#BP) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn overflow_exception(stack_frame: ExceptionStackFrame) {
	error!("Overflow (#OF) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn bound_range_exceeded_exception(stack_frame: ExceptionStackFrame) {
	error!("BOUND Range Exceeded (#BR) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn invalid_opcode_exception(stack_frame: ExceptionStackFrame) {
	error!("Invalid Opcode (#UD) Exception: {:#?}", stack_frame);
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

extern "x86-interrupt" fn invalid_tss_exception(stack_frame: ExceptionStackFrame, _code: u64) {
	error!("Invalid TSS (#TS) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn segment_not_present_exception(
	stack_frame: ExceptionStackFrame,
	_code: u64,
) {
	error!("Segment Not Present (#NP) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn stack_segment_fault_exception(
	stack_frame: ExceptionStackFrame,
	error_code: u64,
) {
	error!(
		"Stack Segment Fault (#SS) Exception: {:#?}, error {:#X}",
		stack_frame, error_code
	);
	scheduler::abort();
}

extern "x86-interrupt" fn general_protection_exception(
	stack_frame: ExceptionStackFrame,
	error_code: u64,
) {
	error!(
		"General Protection (#GP) Exception: {:#?}, error {:#X}",
		stack_frame, error_code
	);
	error!(
		"fs = {:#X}, gs = {:#X}",
		processor::readfs(),
		processor::readgs()
	);
	scheduler::abort();
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

extern "x86-interrupt" fn floating_point_exception(stack_frame: ExceptionStackFrame) {
	error!("Floating-Point Error (#MF) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn alignment_check_exception(stack_frame: ExceptionStackFrame, _code: u64) {
	error!("Alignment Check (#AC) Exception: {:#?}", stack_frame);
	scheduler::abort();
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

extern "x86-interrupt" fn simd_floating_point_exception(stack_frame: ExceptionStackFrame) {
	error!("SIMD Floating-Point (#XM) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn virtualization_exception(stack_frame: ExceptionStackFrame) {
	error!("Virtualization (#VE) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

static IRQ_NAMES: InterruptTicketMutex<HashMap<u8, &'static str, RandomState>> =
	InterruptTicketMutex::new(HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));

pub fn add_irq_name(irq_number: u8, name: &'static str) {
	debug!("Register name \"{}\"  for interrupt {}", name, irq_number);
	IRQ_NAMES.lock().insert(32 + irq_number, name);
}

fn get_irq_name(irq_number: u8) -> Option<&'static str> {
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

	pub fn inc(&self, pos: u8) {
		self.counters[usize::from(pos)].fetch_add(1, Ordering::Relaxed);
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
