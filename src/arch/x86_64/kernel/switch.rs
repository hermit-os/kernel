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
/// The child's saved context has this function as its "return address".
/// When the child is scheduled it restores context and `ret`s here, returning 1 (true).
/// Returns the child's kernel-stack top minus the marker size.
/// Used by `fork_child_start` to locate the saved user RSP.
#[cfg(all(feature = "common-os", feature = "fork"))]
extern "C" fn get_kernel_stack_top() -> usize {
	use crate::arch::x86_64::kernel::core_local::core_scheduler;
	use crate::arch::x86_64::kernel::scheduler::TaskStacks;

	let task = core_scheduler().get_current_task();
	let borrowed = task.borrow();
	(borrowed.stacks.get_kernel_stack()
		+ borrowed.stacks.get_kernel_stack_size() as u64
		- TaskStacks::MARKER_SIZE as u64)
		.as_usize()
}

/// Entry point for the child task after a fork.
///
/// `switch_to_task` restores the saved context and `ret`s here.  Instead of
/// unwinding the whole kernel call-chain (which is fragile), we jump directly
/// to user space:
///   • rax = 0  (fork returns 0 in the child)
///   • rsp, rcx (user RIP), r11 (user RFLAGS) are read back from the user
///     stack that `syscall_handler` prepared before the fork syscall.
///   • swapgs restores the user GS base.
///   • sysretq returns to user space.
#[cfg(all(feature = "common-os", feature = "fork"))]
#[unsafe(naked)]
extern "C" fn fork_child_start() {
	use core::arch::naked_asm;
	naked_asm!(
		// rsp currently points somewhere inside the copied kernel stack.
		// The user RSP (after the 14 user-register pushes) was saved by
		// syscall_handler via `push rcx` right at the top of the kernel stack.
		// Retrieve it: kernel_stack_top - 8.
		"call {get_kernel_stack_top}",    // rax = kernel_stack_top
		"mov  rsp, [rax - 8]",           // rsp  = user_rsp (after 14 pushes on user stack)
		// Restore the 14 user registers that syscall_handler pushed:
		//   r15, r14, r13, r12, r11, r10, r9, r8, rdi, rsi, rbp, rbx, rdx, rcx
		// (rcx will hold the user-space return address for sysretq)
		"pop  r15",
		"pop  r14",
		"pop  r13",
		"pop  r12",
		"pop  r11",   // r11 = user RFLAGS (saved by syscall instruction)
		"pop  r10",
		"pop  r9",
		"pop  r8",
		"pop  rdi",
		"pop  rsi",
		"pop  rbp",
		"pop  rbx",
		"pop  rdx",
		"pop  rcx",   // rcx = user-space return address (saved by syscall instruction)
		// fork() returns 0 in the child
		"xor  eax, eax",
		"swapgs",
		"sysretq",
		get_kernel_stack_top = sym get_kernel_stack_top,
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
		get_current_stack_addr    = sym get_current_stack_addr,
	);
}
