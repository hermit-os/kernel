use core::arch::naked_asm;

use hermit_entry::Entry;
use hermit_entry::boot_info::RawBootInfo;

use crate::KERNEL_STACK_SIZE;
use crate::kernel::pre_init;
use crate::kernel::scheduler::TaskStacks;

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start(_boot_info: Option<&'static RawBootInfo>, cpu_id: u32) -> ! {
	// boot_info is in the `rdi` register

	// validate signatures
	// `_Start` is compatible to `Entry`
	{
		unsafe extern "C" fn _entry(_boot_info: &'static RawBootInfo, _cpu_id: u32) -> ! {
			unreachable!()
		}
		pub type _Start =
			unsafe extern "C" fn(boot_info: Option<&'static RawBootInfo>, cpu_id: u32) -> !;
		const _ENTRY: Entry = _entry;
		const _START: _Start = _start;
		const _PRE_INIT: _Start = pre_init;
	}

	naked_asm!(
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
	)
}
