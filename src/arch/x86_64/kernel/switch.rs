use crate::set_current_kernel_stack;

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
			push_fs!()
		)
	};
}

macro_rules! restore_context {
	() => {
		concat!(
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
			ret
			"#
		)
	};
}

#[naked]
pub unsafe extern "C" fn switch_to_task(_old_stack: *mut usize, _new_stack: usize) {
	// `old_stack` is in `rdi` register
	// `new_stack` is in `rsi` register

	asm!(
		save_context!(),
		// Store the old `rsp` behind `old_stack`
		"mov [rdi], rsp",
		// Set `rsp` to `new_stack`
		"mov rsp, rsi",
		// Set task switched flag
		"mov rax, cr0",
		"or rax, 8",
		"mov cr0, rax",
		// Set stack pointer in TSS
		"call {set_current_kernel_stack}",
		restore_context!(),
		set_current_kernel_stack = sym set_current_kernel_stack,
		options(noreturn)
	);
}

/// Performa a context switch to an idle task or a task, which alread is owner
/// of the FPU.
#[naked]
pub unsafe extern "C" fn switch_to_fpu_owner(_old_stack: *mut usize, _new_stack: usize) {
	// `old_stack` is in `rdi` register
	// `new_stack` is in `rsi` register

	asm!(
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
		options(noreturn),
	);
}
