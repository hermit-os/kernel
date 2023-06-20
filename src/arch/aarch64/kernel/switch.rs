use core::arch::asm;
use core::{mem, ptr};

macro_rules! kernel_function_impl {
	($kernel_function:ident($($arg:ident: $A:ident),*) { $($operands:tt)* }) => {
		/// Executes `f` on the kernel stack.
		#[allow(dead_code)]
		pub fn $kernel_function<R, $($A),*>(f: unsafe extern "C" fn($($A),*) -> R, $($arg: $A),*) -> R {
			unsafe {
				assert!(mem::size_of::<R>() <= mem::size_of::<usize>());

				$(
					assert!(mem::size_of::<$A>() <= mem::size_of::<usize>());
					let $arg = {
						let mut reg = 0_usize;
						// SAFETY: $A is smaller than usize and directly fits in a register
						// Since f takes $A as argument via C calling convention, any opper bytes do not matter.
						ptr::write(&mut reg as *mut _ as _, $arg);
						reg
					};
				)*

				let ret: u64;
				asm!(
					// Switch to kernel stack
					"msr spsel, {l1}",

					// To make sure, Rust manages the stack in `f` correctly,
					// we keep all arguments and return values in registers
					// until we switch the stack back. Thus follows the sizing
					// requirements for arguments and return types.
					"blr {f}",

					// Switch back to user stack
					"msr spsel, {l0}",

					l0 = const 0,
					l1 = const 1,
					f = in(reg) f,

					$($operands)*

					// Return argument in x0
					lateout("x0") ret,

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
	in("x0") arg1,
});

kernel_function_impl!(kernel_function2(arg1: A1, arg2: A2) {
	in("x0") arg1,
	in("x1") arg2,
});

kernel_function_impl!(kernel_function3(arg1: A1, arg2: A2, arg3: A3) {
	in("x0") arg1,
	in("x1") arg2,
	in("x2") arg3,
});

kernel_function_impl!(kernel_function4(arg1: A1, arg2: A2, arg3: A3, arg4: A4) {
	in("x0") arg1,
	in("x1") arg2,
	in("x2") arg3,
	in("x3") arg4,
});

kernel_function_impl!(kernel_function5(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5) {
	in("x0") arg1,
	in("x1") arg2,
	in("x2") arg3,
	in("x3") arg4,
	in("x4") arg5,
});

kernel_function_impl!(kernel_function6(arg1: A1, arg2: A2, arg3: A3, arg4: A4, arg5: A5, arg6: A6) {
	in("x0") arg1,
	in("x1") arg2,
	in("x2") arg3,
	in("x3") arg4,
	in("x4") arg5,
	in("x5") arg6,
});
