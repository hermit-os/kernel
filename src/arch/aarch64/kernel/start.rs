use crate::arch::aarch64::kernel::serial::SerialPort;
use crate::arch::aarch64::kernel::{
	get_processor_count, scheduler::TaskStacks, BootInfo, BOOT_INFO,
};
use crate::KERNEL_STACK_SIZE;

extern "C" {
	static vector_table: u8;
}

/// Entrypoint - Initialize Stack pointer and Exception Table
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
	asm!(
		 "mov x1, x0",
		 "add x1, x1, {current_stack_address_offset}",
		 "ldr x2, [x1]",   /* Previous version subtracted 0x10 from End, so I'm doing this too. Not sure why though. COMMENT from SL: This is a habit of mine. I always start 0x10 bytes before the end of the stack. */
		 "mov x3, {stack_top_offset}",
		 "add x2, x2, x3",
		 "mov sp, x2",
		 /* reset thread id registers */
		 "msr tpidr_el1, xzr",
		 /* set system control register */
		 "ldr x3, ={sctlr}",
		 "msr sctlr_el1, x3",
		 /* Reset debug controll register */
		 "msr mdscr_el1, xzr",
		 /* set exception table */
		 "adrp x4, {vector_table}",
		 "add  x4, x4, #:lo12:{vector_table}",
		 "msr vbar_el1, x4",
		 "adrp x4, {pre_init}",
		 "add  x4, x4, #:lo12:{pre_init}",
		 "br x4",
		 current_stack_address_offset = const BootInfo::current_stack_address_offset(),
		 stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
		 sctlr = const 0x4D5D91Cisize,
		 vector_table = sym vector_table,
		 pre_init = sym pre_init,
		 options(noreturn),
	 )
}

#[inline(never)]
#[no_mangle]
unsafe fn pre_init(boot_info: &'static mut BootInfo) -> ! {
	BOOT_INFO = boot_info as *mut BootInfo;

	if boot_info.cpu_online == 0 {
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				//processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}
