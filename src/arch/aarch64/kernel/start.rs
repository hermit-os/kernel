use core::arch::asm;

use crate::arch::aarch64::kernel::serial::SerialPort;
use crate::arch::aarch64::kernel::{
	get_processor_count, scheduler::TaskStacks, BootInfo, BOOT_INFO,
};
use crate::KERNEL_STACK_SIZE;

extern "C" {
	static vector_table: u8;
}

// TCR flags
const TCR_IRGN_WBWA: u64 = ((1) << 8) | ((1) << 24);
const TCR_ORGN_WBWA: u64 = ((1) << 10) | ((1) << 26);
const TCR_SHARED: u64 = ((3) << 12) | ((3) << 28);
const TCR_TBI0: u64 = 1 << 37;
const TCR_TBI1: u64 = 1 << 38;
const TCR_ASID16: u64 = 1 << 36;
const TCR_TG1_64K: u64 = 3 << 30;
const TCR_TG1_16K: u64 = 1 << 30;
const TCR_TG1_4K: u64 = 0 << 30;
const TCR_FLAGS: u64 = TCR_IRGN_WBWA | TCR_ORGN_WBWA | TCR_SHARED;

/// Number of virtual address bits for 4KB page
const VA_BITS: u64 = 48;

// Available memory types
#[allow(non_upper_case_globals)]
const MT_DEVICE_nGnRnE: u64 = 0;
#[allow(non_upper_case_globals)]
const MT_DEVICE_nGnRE: u64 = 1;
const MT_DEVICE_GRE: u64 = 2;
const MT_NORMAL_NC: u64 = 3;
const MT_NORMAL: u64 = 4;

const fn mair(attr: u64, mt: u64) -> u64 {
	attr << (mt * 8)
}

/// Entrypoint - Initialize Stack pointer and Exception Table
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
	asm!(
		// determine stack address
		"mov x1, x0",
		"add x1, x1, {current_stack_address_offset}",
		"ldr x2, [x1]",
		"mov x3, {stack_top_offset}",
		"add x2, x2, x3",
		"mov sp, x2",
		"adrp x4, {pre_init}",
		"add  x4, x4, #:lo12:{pre_init}",
		"br x4",
		current_stack_address_offset = const BootInfo::current_stack_address_offset(),
		stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
		pre_init = sym pre_init,
		options(noreturn),
	)
}

#[inline(never)]
#[no_mangle]
unsafe fn pre_init(boot_info: &'static mut BootInfo) -> ! {
	BOOT_INFO = boot_info as *mut BootInfo;

	// set exception table
	asm!(
		"adrp x4, {vector_table}",
		"add  x4, x4, #:lo12:{vector_table}",
		"msr vbar_el1, x4",
		vector_table = sym vector_table,
		out("x4") _,
		options(nostack, nomem),
	);

	// Memory barrier
	asm!("dsb sy", options(nostack),);

	if boot_info.cpu_online == 0 {
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				crate::arch::processor::halt()
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}
