use core::arch::asm;
use core::mem;

use super::core_local::CoreLocal;
use crate::syscalls::table::SYSHANDLER_TABLE;

#[no_mangle]
#[naked]
pub(crate) unsafe extern "C" fn syscall_handler() -> ! {
	unsafe {
		asm!(
			// save context, see x86_64 ABI
			"push rcx",
			"push rdx",
			"push rsi",
			"push rdi",
			"push r8",
			"push r9",
			"push r10",
			"push r11",
			// switch to kernel stack
			"swapgs",
			"mov rcx, rsp",
			"mov rsp, gs:{core_local_kernel_stack}",
			// save user stack pointer
			"push rcx",
			// copy 4th argument to rcx to adhere x86_64 ABI
			"mov rcx, r10",
			"sti",
			"mov r10, qword ptr [rip + {table}@GOTPCREL]",
			"call [r10 + 8*rax]",
			"cli",
			// restore user stack pointer
			"pop rcx",
			"mov rsp, rcx",
			"swapgs",
			// restore context, see x86_64 ABI
			"pop r11",
			"pop r10",
			"pop r9",
			"pop r8",
			"pop rdi",
			"pop rsi",
			"pop rdx",
			"pop rcx",
			"sysretq",
			core_local_kernel_stack = const mem::offset_of!(CoreLocal, kernel_stack),
			table = sym SYSHANDLER_TABLE,
			options(noreturn)
		);
	}
}
