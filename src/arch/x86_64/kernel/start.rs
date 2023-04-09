use core::arch::asm;

use hermit_entry::boot_info::RawBootInfo;
use hermit_entry::Entry;

use crate::kernel::pre_init;
use crate::kernel::scheduler::TaskStacks;
use crate::KERNEL_STACK_SIZE;

#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start(_boot_info: &'static RawBootInfo, cpu_id: u32) -> ! {
	// boot_info is in the `rdi` register

	// validate signatures
	const _START: Entry = _start;
	const _PRE_INIT: Entry = pre_init;

	unsafe {
		asm!(
			// use core::sync::atomic::{AtomicU32, Ordering};
			//
			// pub static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);
			//
			// while CPU_ONLINE.load(Ordering::Acquire) != this {
			//     core::hint::spin_loop();
			// }
			"mov rax, qword ptr [rip + {cpu_online}@GOTPCREL]",
			"2:",
			"mov ecx, dword ptr [rax]",
			"cmp ecx, esi",
			"je 3f",
			"pause",
			"jmp 2b",
			"3:",

			// Overwrite RSP if `CURRENT_STACK_ADDRESS != 0`
			"mov rax, qword ptr [rip + {current_stack_address}@GOTPCREL]",
			"mov rax, qword ptr [rax]",
			"test rax, rax",
			"cmovne rsp, rax",
			"mov rax, qword ptr [rip + {current_stack_address}@GOTPCREL]",
			"mov qword ptr [rax], rsp",

			// Add top stack offset
			"add rsp, {stack_top_offset}",

			// Jump into Rust code
			"jmp {pre_init}",

			cpu_online = sym super::CPU_ONLINE,
			current_stack_address = sym super::CURRENT_STACK_ADDRESS,
			stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
			pre_init = sym pre_init,
			options(noreturn)
		)
	}
}
