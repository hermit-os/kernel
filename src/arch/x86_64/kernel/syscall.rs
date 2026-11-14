use core::arch::naked_asm;

use crate::syscalls::table::SYSHANDLER_TABLE;

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn syscall_handler() -> ! {
	naked_asm!(
		"swapgs",
		// switch to kernel stack
		"mov gs:{core_local_user_stack}, rsp",
		"mov rsp, gs:{core_local_kernel_stack}",
		// save context
		"push rcx",
		"push rdx",
		"push rbx",
		"push rbp",
		"push rsi",
		"push rdi",
		"push r8",
		"push r9",
		"push r10",
		"push r11",
		"push r12",
		"push r13",
		"push r14",
		"push r15",
		// Keep the user RSP on the per-task kernel stack: the per-CPU slot is
		// overwritten by syscalls of other tasks while this one is blocked.
		"mov rcx, gs:{core_local_user_stack}",
		"push rcx",
		"sti",
		// copy 4th argument to rcx to adhere x86_64 ABI
		"mov rcx, r10",
		// call system call
		"mov r10, [rip + {table}@GOTPCREL]",
		"call [r10 + 8*rax]",
		"cli",
		// Move the saved user RSP back into the per-CPU slot.
		"pop rcx",
		"mov gs:{core_local_user_stack}, rcx",
		// restore context (without rax)
		"pop r15",
		"pop r14",
		"pop r13",
		"pop r12",
		"pop r11",
		"pop r10",
		"pop r9",
		"pop r8",
		"pop rdi",
		"pop rsi",
		"pop rbp",
		"pop rbx",
		"pop rdx",
		"pop rcx",
		"mov rsp, gs:{core_local_user_stack}",
		"swapgs",
		"sysretq",
		core_local_user_stack = const core::mem::offset_of!(super::core_local::CoreLocal, user_stack),
		core_local_kernel_stack = const core::mem::offset_of!(super::core_local::CoreLocal, kernel_stack),
		table = sym SYSHANDLER_TABLE,
	);
}
