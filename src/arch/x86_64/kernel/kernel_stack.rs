use core::mem;

use crate::core_local::CoreLocal;

#[unsafe(naked)]
pub unsafe extern "C" fn call_with_stack(
	rdi: usize,
	rsi: usize,
	rdx: usize,
	rcx: usize,
	r8: usize,
	r9: usize,
	f: unsafe extern "C" fn(
		rdi: usize,
		rsi: usize,
		rdx: usize,
		rcx: usize,
		r8: usize,
		r9: usize,
	) -> usize,
	stack_ptr: *mut (),
) -> usize {
	core::arch::naked_asm!(
		// Save user stack pointer and switch to supplied stack
		"cli",
		"push r12",
		"mov r12, rsp",
		"mov rsp, [rsp + 24]",
		"sti",
		// Call f
		"call [r12 + 16]",
		// Switch back to previous stack
		"cli",
		"mov rsp, r12",
		"pop r12",
		"sti",
		"ret",
	)
}

macro_rules! kernel_function_impl {
	($kernel_function:ident($($arg:ident: $A:ident),*; $($z:ident: usize),*)) => {
		/// Executes `f` on the kernel stack.
		#[allow(dead_code)]
		pub unsafe fn $kernel_function<R, $($A),*>(f: unsafe extern "C" fn($($A),*) -> R, $($arg: $A),*) -> R {
			unsafe {
				$(
					assert!(mem::size_of::<$A>() <= mem::size_of::<usize>());
				)*
				assert!(mem::size_of::<R>() <= mem::size_of::<usize>());

				let call_with_stack = mem::transmute::<*const (), unsafe extern "C" fn(
					$($arg: $A,)*
					$($z: usize,)*
					f: unsafe extern "C" fn(
						$($arg: $A,)*
					) -> R,
					stack_ptr: *mut (),
				) -> R>(call_with_stack as *const ());

				$(
					let $z = 0usize;
				)*

				let kernel_stack = CoreLocal::get().kernel_stack.get().cast();

				call_with_stack(
					$($arg,)*
					$($z,)*
					f,
					kernel_stack,
				)
			}
		}
	};
}

kernel_function_impl!(kernel_function0(; z1: usize, z2: usize, z3: usize, z4: usize, z5: usize, z6: usize));
kernel_function_impl!(kernel_function1(arg1: A1; z2: usize, z3: usize, z4: usize, z5: usize, z6: usize));
kernel_function_impl!(kernel_function2(arg1: A1, arg2: A2; z3: usize, z4: usize, z5: usize, z6: usize));
kernel_function_impl!(kernel_function3(arg1: A1, arg2: A2, arg3: A3; z4: usize, z5: usize, z6: usize));
kernel_function_impl!(kernel_function4(arg1: A1, arg2: A2, arg3: A3, arg4: A4; z5: usize, z6: usize));
kernel_function_impl!(kernel_function5(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5; z6: usize));
kernel_function_impl!(kernel_function6(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5, arg6: A6; ));
