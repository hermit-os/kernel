use core::arch::asm;

use hermit_entry::RawBootInfo;

use crate::{
	kernel::{pre_init, scheduler::TaskStacks},
	KERNEL_STACK_SIZE,
};

#[no_mangle]
#[naked]
pub extern "C" fn _start(_boot_info: *const RawBootInfo) -> ! {
	// boot_info is in the `rdi` register

	// validate signature
	const _F: unsafe fn(*const RawBootInfo) -> ! = pre_init;

	unsafe {
		asm!(
			// initialize stack pointer
			"mov rsp, [rdi + {current_stack_address_offset}]",
			"add rsp, {stack_top_offset}",
			"mov rbp, rsp",
			"call {pre_init}",
			current_stack_address_offset = const RawBootInfo::current_stack_address_offset(),
			stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
			pre_init = sym pre_init,
			options(noreturn)
		)
	}
}
