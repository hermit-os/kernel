use core::mem;

use crate::core_local::CoreLocal;

type Reg = core::mem::MaybeUninit<usize>;

#[unsafe(naked)]
pub unsafe extern "C" fn call_with_stack(
	rdi: Reg,
	rsi: Reg,
	rdx: Reg,
	rcx: Reg,
	r8: Reg,
	r9: Reg,
	f: unsafe extern "C" fn(rdi: Reg, rsi: Reg, rdx: Reg, rcx: Reg, r8: Reg, r9: Reg) -> Reg,
	stack_ptr: *mut (),
) -> Reg {
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
	($kernel_function:ident($($arg:ident: $A:ident),*; $($z:ident: Reg),*)) => {
		/// Executes `f` on the kernel stack.
		#[allow(dead_code)]
		#[inline]
		pub unsafe extern "C" fn $kernel_function<R, $($A),*>($($arg: $A,)* f: unsafe extern "C" fn($($A),*) -> R) -> R {
			unsafe {
				$(
					assert!(mem::size_of::<$A>() <= mem::size_of::<Reg>());
				)*
				assert!(mem::size_of::<R>() <= mem::size_of::<Reg>());

				let call_with_stack = mem::transmute::<*const (), unsafe extern "C" fn(
					$($arg: $A,)*
					$($z: Reg,)*
					f: unsafe extern "C" fn(
						$($arg: $A,)*
					) -> R,
					stack_ptr: *mut (),
				) -> R>(call_with_stack as *const ());

				$(
					let $z = Reg::uninit();
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

kernel_function_impl!(kernel_function0(; u1: Reg, u2: Reg, u3: Reg, u4: Reg, u5: Reg, u6: Reg));
kernel_function_impl!(kernel_function1(arg1: A1; u2: Reg, u3: Reg, u4: Reg, u5: Reg, u6: Reg));
kernel_function_impl!(kernel_function2(arg1: A1, arg2: A2; u3: Reg, u4: Reg, u5: Reg, u6: Reg));
kernel_function_impl!(kernel_function3(arg1: A1, arg2: A2, arg3: A3; u4: Reg, u5: Reg, u6: Reg));
kernel_function_impl!(kernel_function4(arg1: A1, arg2: A2, arg3: A3, arg4: A4; u5: Reg, u6: Reg));
kernel_function_impl!(kernel_function5(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5; u6: Reg));
kernel_function_impl!(kernel_function6(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5, arg6: A6; ));
