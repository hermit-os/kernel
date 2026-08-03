use core::arch::naked_asm;
use core::sync::atomic::Ordering;

use crate::arch::kernel::CURRENT_BOOT_ID;
use crate::arch::riscv64::kernel::CURRENT_STACK_ADDRESS;
use crate::config::KERNEL_STACK_SIZE;

#[unsafe(naked)]
pub unsafe extern "C" fn smp_start(hart_id: usize) -> ! {
	naked_asm!(
		// Use stack pointer from `CURRENT_STACK_ADDRESS` if set
		"ld t0, {current_stack_pointer}",
		"beqz t0, 2f",
		"li t1, {top_offset}",
		"add t0, t0, t1",
		"mv sp, t0",
		"2:",

		"j {smp_start_rust}",
		current_stack_pointer = sym CURRENT_STACK_ADDRESS,
		top_offset = const KERNEL_STACK_SIZE,
		smp_start_rust = sym smp_start_rust,
	)
}

unsafe extern "C" fn smp_start_rust(hart_id: usize) -> ! {
	CURRENT_BOOT_ID.store(hart_id as u32, Ordering::Relaxed);

	crate::application_processor_main();
}
