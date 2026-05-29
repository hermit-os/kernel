use core::arch::naked_asm;

use x86_64::registers::control::Cr0Flags;

use crate::arch::kernel::gdt::set_current_kernel_stack;

#[cfg(not(feature = "common-os"))]
macro_rules! push_gs {
	() => {
		r#"
		"#
	};
}

#[cfg(not(feature = "common-os"))]
macro_rules! pop_gs {
	() => {
		r#"
		"#
	};
}

#[cfg(all(feature = "fsgsbase", feature = "common-os"))]
macro_rules! push_gs {
	() => {
		r#"
		rdfsbase rax
		push rax
		"#
	};
}

#[cfg(all(feature = "fsgsbase", feature = "common-os"))]
macro_rules! pop_gs {
	() => {
		r#"
		pop rax
		wrfsbase rax
		"#
	};
}

#[cfg(all(not(feature = "fsgsbase"), feature = "common-os"))]
macro_rules! push_gs {
	() => {
		r#"
		mov ecx, 0xc0000101 // Kernel GS.Base Model Specific Register
		rdmsr
		sub rsp, 8
		mov [rsp+4], edx
		mov [rsp], eax
		"#
	};
}

#[cfg(all(not(feature = "fsgsbase"), feature = "common-os"))]
macro_rules! pop_gs {
	() => {
		r#"
		mov ecx, 0xc0000101 // Kernel GS.Base Model Specific Register
		mov edx, [rsp+4]
		mov eax, [rsp]
		add rsp, 8
		wrmsr
		"#
	};
}

#[cfg(feature = "fsgsbase")]
macro_rules! push_fs {
	() => {
		r#"
		rdfsbase rax
		push rax
		"#
	};
}

#[cfg(feature = "fsgsbase")]
macro_rules! pop_fs {
	() => {
		r#"
		pop rax
		wrfsbase rax
		"#
	};
}

#[cfg(not(feature = "fsgsbase"))]
macro_rules! push_fs {
	() => {
		r#"
		mov ecx, 0xc0000100 // FS.Base Model Specific Register
		rdmsr
		sub rsp, 8
		mov [rsp+4], edx
		mov [rsp], eax
		"#
	};
}

#[cfg(not(feature = "fsgsbase"))]
macro_rules! pop_fs {
	() => {
		r#"
		mov ecx, 0xc0000100 // FS.Base Model Specific Register
		mov edx, [rsp+4]
		mov eax, [rsp]
		add rsp, 8
		wrmsr
		"#
	};
}

macro_rules! save_context {
	() => {
		concat!(
			r#"
			pushfq
			push rax
			push rcx
			push rdx
			push rbx
			push rbp
			push rsi
			push rdi
			push r8
			push r9
			push r10
			push r11
			push r12
			push r13
			push r14
			push r15
			"#,
			push_fs!(),
			push_gs!()
		)
	};
}

macro_rules! restore_context {
	() => {
		concat!(
			restore_context_without_return!(),
			r#"
			ret
			"#
		)
	};
}

macro_rules! restore_context_without_return {
	() => {
		concat!(
			pop_gs!(),
			pop_fs!(),
			r#"
			pop r15
			pop r14
			pop r13
			pop r12
			pop r11
			pop r10
			pop r9
			pop r8
			pop rdi
			pop rsi
			pop rbp
			pop rbx
			pop rdx
			pop rcx
			pop rax
			popfq
			"#
		)
	};
}

#[unsafe(naked)]
pub(crate) unsafe extern "C" fn switch_to_task(_old_stack: *mut usize, _new_stack: usize) {
	// `old_stack` is in `rdi` register
	// `new_stack` is in `rsi` register

	naked_asm!(
		save_context!(),
		// Store the old `rsp` behind `old_stack`
		"mov [rdi], rsp",
		// Set `rsp` to `new_stack`
		"mov rsp, rsi",
		// Set task switched flag
		"mov rax, cr0",
		"or rax, {task_switched}",
		"mov cr0, rax",
		// Set stack pointer in TSS
		"call {set_current_kernel_stack}",
		restore_context!(),
		task_switched = const Cr0Flags::TASK_SWITCHED.bits(),
		set_current_kernel_stack = sym set_current_kernel_stack,
	);
}

/// Performs a context switch to an idle task or a task, which already is owner
/// of the FPU.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn switch_to_fpu_owner(_old_stack: *mut usize, _new_stack: usize) {
	// `old_stack` is in `rdi` register
	// `new_stack` is in `rsi` register

	naked_asm!(
		save_context!(),
		// Store the old `rsp` behind `old_stack`
		"mov [rdi], rsp",
		// Set `rsp` to `new_stack`
		"mov rsp, rsi",
		// Don't set task switched flag, as we switch to fpu owner.
		// Set stack pointer in TSS
		"call {set_current_kernel_stack}",
		restore_context!(),
		set_current_kernel_stack = sym set_current_kernel_stack,
	);
}

// ── Fork support ─────────────────────────────────────────────────────────────

/// Entry point for the child task after a fork.
///
/// `switch_to_task` restores the saved context and `ret`s here. Instead
/// of unwinding the whole kernel call-chain (which is fragile), we jump
/// directly to user space, mirroring `syscall_handler`'s return path:
///   • The 14 user-side registers (`rcx`, `rdx`, `rbx`, `rbp`, `rsi`,
///     `rdi`, `r8`..`r15`) sit at the top of the child's kernel stack
///     because `copy_kernel_stack_to` copied them across when the
///     parent ran `prepare_fork_child_stack`. `rcx` holds the user
///     RIP and `r11` the user RFLAGS — the `syscall` instruction
///     stashed them there before `syscall_handler` even started.
///   • The user RSP for the child is in the unused MARKER_SIZE pad of
///     the kernel stack at `kernel_top - 8`.
///     `prepare_fork_child_stack` writes it there from the per-CPU
///     `user_stack` slot at fork time, so we get a snapshot that
///     survives any other syscall taking place between the parent's
///     fork() and the child being scheduled.
///   • `rax = 0` (fork returns 0 in the child), `swapgs` restores the
///     user GS base, `sysretq` jumps back to user mode.
#[cfg(all(feature = "common-os", feature = "fork"))]
#[unsafe(naked)]
extern "C" fn fork_child_start() {
	use core::arch::naked_asm;
	naked_asm!(
		// Position rsp at the top of the saved-register block.
		// `gs:{kernel_stack}` = kernel_top - MARKER_SIZE = kernel_top - 16.
		// After 14 register pushes by syscall_handler rsp was
		// gs:{kernel_stack} - 14*8.
		"mov  rsp, gs:{core_local_kernel_stack}",
		"sub  rsp, 14 * 8",
		// Pop the 14 saved registers (rcx = user RIP, r11 = user RFLAGS).
		"pop  r15",
		"pop  r14",
		"pop  r13",
		"pop  r12",
		"pop  r11",
		"pop  r10",
		"pop  r9",
		"pop  r8",
		"pop  rdi",
		"pop  rsi",
		"pop  rbp",
		"pop  rbx",
		"pop  rdx",
		"pop  rcx",
		// fork() returns 0 in the child.
		"xor  eax, eax",
		// Load the per-task user_rsp from kernel_top - 8 (the unused
		// MARKER_SIZE pad above the marker; `prepare_fork_child_stack`
		// wrote it there). gs:{kernel_stack} holds kernel_top - 16,
		// so the slot we want is at [kernel_stack + 8]. Go via rsp
		// itself — we are about to overwrite it anyway and must not
		// clobber rax (the fork return value).
		"mov  rsp, gs:{core_local_kernel_stack}",
		"mov  rsp, [rsp + 8]",
		"swapgs",
		"sysretq",
		core_local_kernel_stack = const core::mem::offset_of!(super::core_local::CoreLocal, kernel_stack),
	);
}

/// Returns the base virtual address of the current task's stack allocation.
/// Used to calculate the offset for the child's stack pointer.
#[cfg(all(feature = "common-os", feature = "fork"))]
extern "C" fn get_current_stack_addr() -> usize {
	use crate::arch::x86_64::kernel::core_local::core_scheduler;
	core_scheduler()
		.get_current_task()
		.borrow()
		.stacks
		.get_stack_virt_addr()
		.as_usize()
}

/// C-callable wrapper: copy the current root page table and return the new PML4 physical address.
#[cfg(all(feature = "common-os", feature = "fork"))]
extern "C" fn copy_current_root_page_table() -> usize {
	crate::arch::x86_64::mm::copy_current_root_page_table()
}

/// C-callable wrapper: copy the current kernel stack to `stack_addr`.
#[cfg(all(feature = "common-os", feature = "fork"))]
extern "C" fn copy_kernel_stack_to(stack_addr: usize) {
	crate::arch::x86_64::mm::copy_kernel_stack_to(stack_addr);
}

/// Snapshot the current user RSP into the child's kernel stack so that
/// `fork_child_start` can recover it via `sysretq` regardless of what
/// other syscalls do to the per-CPU `user_stack` slot in the meantime.
///
/// The slot we use sits at `child_kernel_top - 8`, i.e. inside the
/// `MARKER_SIZE` pad above the `0xdeadbeef` marker — see [`fork_child_start`]
/// for the matching `mov rsp, [gs:{kernel_stack} + 8]`.
///
/// The parent's user RSP was stashed by `syscall_handler` into
/// `gs:[user_stack]` at syscall entry; we just read it back here.
#[cfg(all(feature = "common-os", feature = "fork"))]
extern "C" fn stash_user_rsp_in_child_stack(new_stack_addr: usize) {
	use crate::arch::x86_64::kernel::core_local::CoreLocal;
	use crate::arch::x86_64::kernel::interrupts::IST_SIZE;
	use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
	use crate::config::DEFAULT_STACK_SIZE;

	let user_rsp: usize;
	unsafe {
		core::arch::asm!(
			"mov {0}, gs:[{1}]",
			out(reg) user_rsp,
			const core::mem::offset_of!(CoreLocal, user_stack),
			options(nostack, preserves_flags),
		);
	}

	let child_kernel_top =
		new_stack_addr + IST_SIZE + 2 * BasePageSize::SIZE as usize + DEFAULT_STACK_SIZE;
	unsafe {
		core::ptr::with_exposed_provenance_mut::<usize>(child_kernel_top - 8).write(user_rsp);
	}
}

/// Prepare the child's stack for a fork.
///
/// On entry (parent):
///   rdi = `stack_pointer`   (*mut usize — receives the child's rsp)
///   rsi = `root_page_table` (*mut usize — receives the child's PML4 phys addr)
///   rdx = `new_stack_addr`  (base virt addr of the child's stack allocation)
///
/// Returns `false` in the parent; the child task's saved context will `ret` to
/// `fork_child_start` which jumps directly back to user space via `sysretq`.
#[cfg(all(feature = "common-os", feature = "fork"))]
#[unsafe(naked)]
pub unsafe extern "C" fn prepare_fork_child_stack(
	_stack_pointer: *mut usize,
	_root_page_table: *mut usize,
	_new_stack_addr: usize,
) -> bool {
	naked_asm!(
		// Push fork_child_start as the child's future return address.
		"lea rax, [rip + {fork_child_start}]",
		"push rax",
		// Save all caller-saved and callee-saved registers.
		save_context!(),
		// Spill the three parameters onto the stack so we can call C functions.
		"push rdi",   // [rsp+16]: stack_pointer ptr
		"push rdx",   // [rsp+8]:  new_stack_addr
		"push rsi",   // [rsp+0]:  root_page_table ptr

		// 1. Copy the kernel stack pages FIRST, so the copied mappings exist in the
		//    current page table before we snapshot it for the child's PML4.
		"mov  rdi, [rsp+8]",              // rdi = new_stack_addr
		"call {copy_kernel_stack_to}",    // copy kernel stack pages

		// 1b. Stash the user RSP into the child's kernel stack at
		//     child_kernel_top - 8, so that fork_child_start can
		//     restore it via sysretq without relying on the per-CPU
		//     `user_stack` slot (which other syscalls may clobber).
		"mov  rdi, [rsp+8]",              // rdi = new_stack_addr
		"call {stash_user_rsp_in_child_stack}",

		// 2. Duplicate the page table (COW) — snapshot now includes the copied stack.
		"call {copy_current_root_page_table}",   // rax = new PML4 phys addr
		"mov  rsi, [rsp]",                       // rsi = root_page_table ptr
		"mov  [rsi], rax",                       // *root_page_table = new PML4

		// 3. Calculate the child's stack pointer.
		//    child_rsp = rsp_after_save_context + (new_stack_addr - current_stack_base)
		"call {get_current_stack_addr}",  // rax = current stack base addr
		"pop  rsi",                       // pop root_page_table ptr (already written)
		"pop  rbx",                       // rbx = new_stack_addr
		"pop  rdi",                       // rdi = stack_pointer ptr
		"sub  rbx, rax",                  // rbx = new_stack_addr - stack_base (offset)
		"add  rbx, rsp",                  // rbx = rsp_after_save_context + offset = child rsp
		"mov  [rdi], rbx",                // *stack_pointer = child_rsp

		// Restore registers (parent continues normally).
		restore_context_without_return!(),
		// Skip the fork_child_start address we pushed at the top.
		"add rsp, 8",
		// Return false (0) — this is the parent.
		"xor rax, rax",
		"ret",
		fork_child_start          = sym fork_child_start,
		copy_current_root_page_table = sym copy_current_root_page_table,
		copy_kernel_stack_to      = sym copy_kernel_stack_to,
		stash_user_rsp_in_child_stack = sym stash_user_rsp_in_child_stack,
		get_current_stack_addr    = sym get_current_stack_addr,
	);
}
