use core::arch::naked_asm;
use core::mem;

use super::core_local::CoreLocal;
use crate::syscalls::table::SYSHANDLER_TABLE;

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn syscall_handler() -> ! {
	naked_asm!(
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
		"swapgs",
		"mfence",
		// switch to kernel stack
		"mov rcx, rsp",
		"mov rsp, gs:{core_local_kernel_stack}",
		"sti",
		// save user stack pointer
		"push rcx",
		// copy 4th argument to rcx to adhere x86_64 ABI
		"mov rcx, r10",
		// call system call
		"mov r10, qword ptr [rip + {table}@GOTPCREL]",
		"call [r10 + 8*rax]",
		// restore user stack pointer
		"pop rcx",
		"mov rsp, rcx",
		"cli",
		"mfence",
		"swapgs",
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
		"sysretq",
		core_local_kernel_stack = const mem::offset_of!(CoreLocal, kernel_stack),
		table = sym SYSHANDLER_TABLE,
	);
}
