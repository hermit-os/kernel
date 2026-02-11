use core::mem;

type Reg = mem::MaybeUninit<usize>;

#[unsafe(naked)]
pub unsafe extern "C" fn call_with_kernel_stack(
	x0: Reg,
	x1: Reg,
	x2: Reg,
	x3: Reg,
	x4: Reg,
	x5: Reg,
	f: unsafe extern "C" fn(x0: Reg, x1: Reg, x2: Reg, x3: Reg, x4: Reg, x5: Reg) -> Reg,
) -> Reg {
	core::arch::naked_asm!(
		// Disable IRQs and FIQs while changing stack pointer
		"msr daifset, #0b11",
		// Preserve return address on the stack
		"str x30, [sp, #-16]!",
		// Switch to kernel stack
		"msr spsel, #1",
		// Re-enable IRQs and FIQs
		"msr daifclr, #0b11",
		// Call the function pointer (stored in x6)
		"blr x6",
		// Disable IRQs and FIQs before restoring stack
		"msr daifset, #0b11",
		// Switch back to user stack
		"msr spsel, #0",
		// Restore return address from the stack
		"ldr x30, [sp], 16",
		// Re-enable IRQs and FIQs
		"msr daifclr, #0b11",
		// Return to caller (return value is in x0)
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

				let call_with_kernel_stack = mem::transmute::<*const (), unsafe extern "C" fn(
					$($arg: $A,)*
					$($z: Reg,)*
					f: unsafe extern "C" fn(
						$($arg: $A,)*
					) -> R,
				) -> R>(call_with_kernel_stack as *const ());

				$(
					let $z = Reg::uninit();
				)*

				call_with_kernel_stack(
					$($arg,)*
					$($z,)*
					f,
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
