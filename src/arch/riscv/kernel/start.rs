use core::arch::asm;

use crate::arch::riscv::kernel::{BootInfo, BOOTINFO_MAGIC_NUMBER, BOOT_INFO};
use crate::KERNEL_STACK_SIZE;

//static mut BOOT_STACK: [u8; KERNEL_STACK_SIZE] = [0; KERNEL_STACK_SIZE];

/// Entrypoint - Initalize Stack pointer and Exception Table
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
	asm!(
		"ld sp, (0x48)(a1)",
		"li t0, {top_offset}",
		"add sp, sp, t0",
		//"mv a0, a1",
		"j {pre_init}", //call or j?
		//boot_stack = sym BOOT_STACK,
		top_offset = const KERNEL_STACK_SIZE - 16, /*Previous version subtracted 0x10 from End, so I'm doing this too. Not sure why though */
		pre_init = sym pre_init,
		options(noreturn),
	)
}

#[inline(never)]
#[no_mangle]
unsafe fn pre_init(hart_id: usize, boot_info: &'static mut BootInfo) -> ! {
	BOOT_INFO = boot_info as *mut BootInfo;

	// info!("Welcome to hermit kernel.");
	assert_eq!(boot_info.magic_number, BOOTINFO_MAGIC_NUMBER);

	core::ptr::write_volatile(&mut (*BOOT_INFO).current_boot_id, hart_id as u32);

	if boot_info.cpu_online == 0 {
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}
