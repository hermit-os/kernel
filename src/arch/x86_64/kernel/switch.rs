use core::arch::asm;
use core::{mem, ptr};

use crate::core_local::CoreLocal;
use crate::set_current_kernel_stack;

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
			ret
			"#
		)
	};
}

#[naked]
pub(crate) unsafe extern "C" fn switch_to_task(_old_stack: *mut usize, _new_stack: usize) {
	// `old_stack` is in `rdi` register
	// `new_stack` is in `rsi` register

	unsafe {
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
}

/// Performa a context switch to an idle task or a task, which already is owner
/// of the FPU.
#[naked]
pub(crate) unsafe extern "C" fn switch_to_fpu_owner(_old_stack: *mut usize, _new_stack: usize) {
	// `old_stack` is in `rdi` register
	// `new_stack` is in `rsi` register

	unsafe {
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
}

macro_rules! kernel_function_impl {
	($kernel_function:ident($($arg:ident: $A:ident),*) { $($operands:tt)* }) => {
		/// Executes `f` on the kernel stack.
		#[allow(dead_code)]
		pub unsafe fn $kernel_function<R, $($A),*>(f: unsafe extern "C" fn($($A),*) -> R, $($arg: $A),*) -> R {
			unsafe {
				assert!(mem::size_of::<R>() <= mem::size_of::<usize>());

				$(
					assert!(mem::size_of::<$A>() <= mem::size_of::<usize>());
					let $arg = {
						let mut reg = 0_usize;
						// SAFETY: $A is smaller than usize and directly fits in a register
						// Since f takes $A as argument via C calling convention, any upper bytes do not matter.
						ptr::write(ptr::from_mut(&mut reg) as _, $arg);
						reg
					};
				)*

				let ret: u64;
				asm!(
					// Save user stack pointer and switch to kernel stack
					"cli",
					"mov r12, rsp",
					"mov rsp, {kernel_stack_ptr}",
					"sti",

					// To make sure, Rust manages the stack in `f` correctly,
					// we keep all arguments and return values in registers
					// until we switch the stack back. Thus follows the sizing
					// requirements for arguments and return types.
					"call {f}",

					// Switch back to user stack
					"cli",
					"mov rsp, r12",
					"sti",

					f = in(reg) f,
					kernel_stack_ptr = in(reg) CoreLocal::get().kernel_stack.get(),

					$($operands)*

					// user_stack_ptr saved in r12
					out("r12") _,

					// Return argument in rax
					out("rax") ret,

					clobber_abi("C"),
				);

				// SAFETY: R is smaller than usize and directly fits in rax
				// Since f returns R, we can safely convert ret to R
				mem::transmute_copy(&ret)
			}
		}
	};
}

kernel_function_impl!(kernel_function0() {});

kernel_function_impl!(kernel_function1(arg1: A1) {
	in("rdi") arg1,
});

kernel_function_impl!(kernel_function2(arg1: A1, arg2: A2) {
	in("rdi") arg1,
	in("rsi") arg2,
});

kernel_function_impl!(kernel_function3(arg1: A1, arg2: A2, arg3: A3) {
	in("rdi") arg1,
	in("rsi") arg2,
	in("rdx") arg3,
});

kernel_function_impl!(kernel_function4(arg1: A1, arg2: A2, arg3: A3, arg4: A4) {
	in("rdi") arg1,
	in("rsi") arg2,
	in("rdx") arg3,
	in("rcx") arg4,
});

kernel_function_impl!(kernel_function5(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5) {
	in("rdi") arg1,
	in("rsi") arg2,
	in("rdx") arg3,
	in("rcx") arg4,
	in("r8") arg5,
});

kernel_function_impl!(kernel_function6(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5, arg6: A6) {
	in("rdi") arg1,
	in("rsi") arg2,
	in("rdx") arg3,
	in("rcx") arg4,
	in("r8") arg5,
	in("r9") arg6,
});
