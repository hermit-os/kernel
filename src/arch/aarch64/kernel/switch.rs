use core::arch::asm;

#[inline]
pub unsafe extern "C" fn switch_to_fpu_owner(old_stack: *mut usize, new_stack: usize) {
	switch_to_task(old_stack, new_stack);
}

#[naked]
pub unsafe extern "C" fn switch_to_task(_old_stack: *mut usize, _new_stack: usize) {
	asm!(
		// save general purpose registers
		"stp x29, x30, [sp, #-16]!",
		"stp x27, x28, [sp, #-16]!",
		"stp x25, x26, [sp, #-16]!",
		"stp x23, x24, [sp, #-16]!",
		"stp x21, x22, [sp, #-16]!",
		"stp x19, x20, [sp, #-16]!",
		"stp x17, x18, [sp, #-16]!",
		"stp x15, x16, [sp, #-16]!",
		"stp x13, x14, [sp, #-16]!",
		"stp x11, x12, [sp, #-16]!",
		"stp x9, x10, [sp, #-16]!",
		"stp x7, x8, [sp, #-16]!",
		"stp x5, x6, [sp, #-16]!",
		"stp x3, x4, [sp, #-16]!",
		"stp x1, x2, [sp, #-16]!",
		// save thread id register and process state
		"mrs x22, tpidr_el0",
		"stp x22, x0, [sp, #-16]!",
		"mrs x22, elr_el1",
		"mrs x23, spsr_el1",
		"stp x22, x23, [sp, #-16]!",
		// Store the old `sp` behind `old_stack`
		"mov x24, sp",
		"str x24, [x0]",
		// Set `sp` to `new_stack`
		"mov sp, x1",
		// restore thread id register and process state
		"ldp x22, x23, [sp], #16",
		"msr elr_el1, x22",
		"msr spsr_el1, x23",
		"ldp x22, x0, [sp], #16",
		"msr tpidr_el0, x22",
		// restore general purpose registers
		"ldp x1, x2, [sp], #16",
		"ldp x3, x4, [sp], #16",
		"ldp x5, x6, [sp], #16",
		"ldp x7, x8, [sp], #16",
		"ldp x9, x10, [sp], #16",
		"ldp x11, x12, [sp], #16",
		"ldp x13, x14, [sp], #16",
		"ldp x15, x16, [sp], #16",
		"ldp x17, x18, [sp], #16",
		"ldp x19, x20, [sp], #16",
		"ldp x21, x22, [sp], #16",
		"ldp x23, x24, [sp], #16",
		"ldp x25, x26, [sp], #16",
		"ldp x27, x28, [sp], #16",
		"ldp x29, x30, [sp], #16",
		"ret",
		options(noreturn),
	);
}
