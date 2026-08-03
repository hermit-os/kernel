use x86_64::registers::control::{Cr0, Cr0Flags};

use crate::arch::kernel::CURRENT_STACK_ADDRESS;
use crate::arch::kernel::scheduler::TaskStacks;
use crate::config::KERNEL_STACK_SIZE;

#[unsafe(naked)]
pub unsafe extern "C" fn smp_start() -> ! {
	core::arch::naked_asm!(
		// Overwrite RSP with `CURRENT_STACK_ADDRESS`
		"mov rax, qword ptr [rip + {current_stack_address}@GOTPCREL]",
		"mov rsp, qword ptr [rax]",

		// Add top stack offset
		"add rsp, {stack_top_offset}",

		// Jump into Rust code
		"jmp {smp_start_rust}",

		current_stack_address = sym CURRENT_STACK_ADDRESS,
		stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
		smp_start_rust = sym smp_start_rust,
	)
}

unsafe extern "C" fn smp_start_rust() -> ! {
	// Enable caching
	unsafe {
		Cr0::update(|flags| flags.remove(Cr0Flags::CACHE_DISABLE | Cr0Flags::NOT_WRITE_THROUGH));
	}

	crate::rt::application_processor_main();
}
