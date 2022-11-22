use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use core::arch::asm;

use hermit_sync::InterruptTicketMutex;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::PageFaultErrorCode;

use crate::arch::x86_64::kernel::percore::{core_scheduler, increment_irq_counter};
use crate::arch::x86_64::kernel::{apic, idt, processor};
use crate::scheduler;

static IRQ_NAMES: InterruptTicketMutex<BTreeMap<u32, String>> =
	InterruptTicketMutex::new(BTreeMap::new());

pub use x86_64::instructions::interrupts::{disable, enable, enable_and_hlt as enable_and_wait};
pub use x86_64::structures::idt::InterruptStackFrame as ExceptionStackFrame;

pub fn install() {
	// Set gates to the Interrupt Service Routines (ISRs) for all 32 CPU exceptions.
	// All of them use a dedicated stack per task (IST1) to prevent clobbering the current task stack.
	// Some critical exceptions also get their own stacks to always execute on a known good stack:
	//   - Non-Maskable Interrupt Exception (IST2)
	//   - Double Fault Exception (IST3)
	//   - Machine Check Exception (IST4)
	//
	// Refer to Intel Vol. 3A, 6.14.5 Interrupt Stack Table.
	idt::set_gate(0, divide_error_exception as usize, 1);
	idt::set_gate(1, debug_exception as usize, 1);
	idt::set_gate(2, nmi_exception as usize, 2);
	idt::set_gate(3, breakpoint_exception as usize, 1);
	idt::set_gate(4, overflow_exception as usize, 1);
	idt::set_gate(5, bound_range_exceeded_exception as usize, 1);
	idt::set_gate(6, invalid_opcode_exception as usize, 1);
	idt::set_gate(7, device_not_available_exception as usize, 1);
	idt::set_gate(8, double_fault_exception as usize, 3);
	idt::set_gate(9, coprocessor_segment_overrun_exception as usize, 1);
	idt::set_gate(10, invalid_tss_exception as usize, 1);
	idt::set_gate(11, segment_not_present_exception as usize, 1);
	idt::set_gate(12, stack_segment_fault_exception as usize, 1);
	idt::set_gate(13, general_protection_exception as usize, 1);
	idt::set_gate(14, page_fault_handler as usize, 1);
	idt::set_gate(15, reserved_exception as usize, 1);
	idt::set_gate(16, floating_point_exception as usize, 1);
	idt::set_gate(17, alignment_check_exception as usize, 1);
	idt::set_gate(18, machine_check_exception as usize, 4);
	idt::set_gate(19, simd_floating_point_exception as usize, 1);
	idt::set_gate(20, virtualization_exception as usize, 1);
	idt::set_gate(21, reserved_exception as usize, 1);
	idt::set_gate(22, reserved_exception as usize, 1);
	idt::set_gate(23, reserved_exception as usize, 1);
	idt::set_gate(24, reserved_exception as usize, 1);
	idt::set_gate(25, reserved_exception as usize, 1);
	idt::set_gate(26, reserved_exception as usize, 1);
	idt::set_gate(27, reserved_exception as usize, 1);
	idt::set_gate(28, reserved_exception as usize, 1);
	idt::set_gate(29, reserved_exception as usize, 1);
	idt::set_gate(30, reserved_exception as usize, 1);
	idt::set_gate(31, reserved_exception as usize, 1);

	idt::set_gate(32, unhandled_interrupt0 as usize, 0);
	idt::set_gate(33, unhandled_interrupt1 as usize, 0);
	idt::set_gate(34, unhandled_interrupt2 as usize, 0);
	idt::set_gate(35, unhandled_interrupt3 as usize, 0);
	idt::set_gate(36, unhandled_interrupt4 as usize, 0);
	idt::set_gate(37, unhandled_interrupt5 as usize, 0);
	idt::set_gate(38, unhandled_interrupt6 as usize, 0);
	idt::set_gate(39, unhandled_interrupt7 as usize, 0);
	idt::set_gate(40, unhandled_interrupt8 as usize, 0);
	idt::set_gate(41, unhandled_interrupt9 as usize, 0);
	idt::set_gate(42, unhandled_interrupt10 as usize, 0);
	idt::set_gate(43, unhandled_interrupt11 as usize, 0);
	idt::set_gate(44, unhandled_interrupt12 as usize, 0);
	idt::set_gate(45, unhandled_interrupt13 as usize, 0);
	idt::set_gate(46, unhandled_interrupt14 as usize, 0);
	idt::set_gate(47, unhandled_interrupt15 as usize, 0);
	idt::set_gate(48, unhandled_interrupt16 as usize, 0);
	idt::set_gate(49, unhandled_interrupt17 as usize, 0);
	idt::set_gate(50, unhandled_interrupt18 as usize, 0);
	idt::set_gate(51, unhandled_interrupt19 as usize, 0);
	idt::set_gate(52, unhandled_interrupt20 as usize, 0);
	idt::set_gate(53, unhandled_interrupt21 as usize, 0);
	idt::set_gate(54, unhandled_interrupt22 as usize, 0);
	idt::set_gate(55, unhandled_interrupt23 as usize, 0);
	idt::set_gate(56, unhandled_interrupt24 as usize, 0);
	idt::set_gate(57, unhandled_interrupt25 as usize, 0);
	idt::set_gate(58, unhandled_interrupt26 as usize, 0);
	idt::set_gate(59, unhandled_interrupt27 as usize, 0);
	idt::set_gate(60, unhandled_interrupt28 as usize, 0);
	idt::set_gate(61, unhandled_interrupt29 as usize, 0);
	idt::set_gate(62, unhandled_interrupt30 as usize, 0);
	idt::set_gate(63, unhandled_interrupt31 as usize, 0);

	for i in 64..idt::IDT_ENTRIES {
		idt::set_gate(i as u8, unknown_interrupt as usize, 0);
	}
}

#[no_mangle]
pub extern "C" fn irq_install_handler(irq_number: u32, handler: usize) {
	debug!("Install handler for interrupt {}", irq_number);
	idt::set_gate((32 + irq_number) as u8, handler, 0);
}

pub fn add_irq_name(irq_number: u32, name: &'static str) {
	debug!("Register name \"{}\"  for interrupt {}", name, irq_number);
	IRQ_NAMES.lock().insert(32 + irq_number, name.to_string());
}

pub fn get_irq_name(irq_number: u32) -> Option<String> {
	let name = IRQ_NAMES.lock().get(&irq_number)?.clone();
	Some(name)
}

fn unhandled_interrupt(irq_number: u8) {
	warn!("Receive unhandled interrupt {}", irq_number);
	apic::eoi();
	increment_irq_counter((32 + irq_number).into());
}

extern "x86-interrupt" fn unhandled_interrupt0(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(0);
}

extern "x86-interrupt" fn unhandled_interrupt1(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(1);
}

extern "x86-interrupt" fn unhandled_interrupt2(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(2);
}

extern "x86-interrupt" fn unhandled_interrupt3(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(3);
}

extern "x86-interrupt" fn unhandled_interrupt4(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(4);
}

extern "x86-interrupt" fn unhandled_interrupt5(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(5);
}

extern "x86-interrupt" fn unhandled_interrupt6(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(6);
}

extern "x86-interrupt" fn unhandled_interrupt7(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(7);
}

extern "x86-interrupt" fn unhandled_interrupt8(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(8);
}

extern "x86-interrupt" fn unhandled_interrupt9(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(9);
}

extern "x86-interrupt" fn unhandled_interrupt10(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(10);
}

extern "x86-interrupt" fn unhandled_interrupt11(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(11);
}

extern "x86-interrupt" fn unhandled_interrupt12(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(12);
}

extern "x86-interrupt" fn unhandled_interrupt13(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(13);
}

extern "x86-interrupt" fn unhandled_interrupt14(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(14);
}

extern "x86-interrupt" fn unhandled_interrupt15(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(15);
}

extern "x86-interrupt" fn unhandled_interrupt16(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(16);
}

extern "x86-interrupt" fn unhandled_interrupt17(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(17);
}

extern "x86-interrupt" fn unhandled_interrupt18(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(18);
}

extern "x86-interrupt" fn unhandled_interrupt19(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(19);
}

extern "x86-interrupt" fn unhandled_interrupt20(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(20);
}

extern "x86-interrupt" fn unhandled_interrupt21(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(21);
}

extern "x86-interrupt" fn unhandled_interrupt22(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(22);
}

extern "x86-interrupt" fn unhandled_interrupt23(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(23);
}

extern "x86-interrupt" fn unhandled_interrupt24(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(24);
}

extern "x86-interrupt" fn unhandled_interrupt25(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(25);
}

extern "x86-interrupt" fn unhandled_interrupt26(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(26);
}

extern "x86-interrupt" fn unhandled_interrupt27(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(27);
}

extern "x86-interrupt" fn unhandled_interrupt28(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(28);
}

extern "x86-interrupt" fn unhandled_interrupt29(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(29);
}

extern "x86-interrupt" fn unhandled_interrupt30(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(30);
}

extern "x86-interrupt" fn unhandled_interrupt31(_stack_frame: ExceptionStackFrame) {
	unhandled_interrupt(31);
}

extern "x86-interrupt" fn unknown_interrupt(_stack_frame: ExceptionStackFrame) {
	info!("Receive unknown interrupt");
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

extern "x86-interrupt" fn coprocessor_segment_overrun_exception(stack_frame: ExceptionStackFrame) {
	error!(
		"CoProcessor Segment Overrun (#MF) Exception: {:#?}",
		stack_frame
	);
	scheduler::abort();
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

extern "x86-interrupt" fn floating_point_exception(stack_frame: ExceptionStackFrame) {
	error!("Floating-Point Error (#MF) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn alignment_check_exception(stack_frame: ExceptionStackFrame, _code: u64) {
	error!("Alignment Check (#AC) Exception: {:#?}", stack_frame);
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

extern "x86-interrupt" fn reserved_exception(stack_frame: ExceptionStackFrame) {
	error!("Reserved Exception: {:#?}", stack_frame);
	scheduler::abort();
}

#[derive(Clone, Copy)]
#[repr(align(64))]
pub struct IrqStatistics {
	pub counters: [u64; 256],
}

impl IrqStatistics {
	pub const fn new() -> Self {
		IrqStatistics { counters: [0; 256] }
	}

	pub fn inc(&mut self, pos: usize) {
		self.counters[pos] += 1;
	}
}
