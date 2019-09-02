// Copyright (c) 2017-2018 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[inline(never)]
#[naked]
pub extern "C" fn switch(_old_stack: *mut usize, _new_stack: usize) {
	// rdi = old_stack => the address to store the old rsp
	// rsi = new_stack => stack pointer of the new task

	unsafe {
		asm!(
			// store context
			"pushfq\n\t\
			push %rax\n\t\
			push %rcx\n\t\
			push %rdx\n\t\
			push %rbx\n\t\
			push %rbp\n\t\
			push %rsi\n\t\
			push %rdi\n\t\
			push %r8\n\t\
			push %r9\n\t\
			push %r10\n\t\
			push %r11\n\t\
			push %r12\n\t\
			push %r13\n\t\
			push %r14\n\t\
			push %r15\n\t\
			rdfsbaseq %rax\n\t\
			push %rax\n\t\
			rdgsbaseq %rax\n\t\
			push %rax\n\t\
			// store the old stack pointer in the dereferenced first parameter\n\t\
			// and load the new stack pointer in the second parameter.\n\t\
			mov %rsp, (%rdi)\n\t\
			mov %rsi, %rsp\n\t\
			// Set task switched flag \n\t\
			mov %cr0, %rax\n\t\
			or $$8, %rax\n\t\
			mov %rax, %cr0\n\t\
			// set stack pointer in TSS \n\t\
			call set_current_kernel_stack \n\t\
			// restore context \n\t\
			pop %rax\n\t\
			wrgsbaseq %rax\n\t\
			pop %rax\n\t\
			wrfsbaseq %rax\n\t\
			pop %r15\n\t\
			pop %r14\n\t\
			pop %r13\n\t\
			pop %r12\n\t\
			pop %r11\n\t\
			pop %r10\n\t\
			pop %r9\n\t\
			pop %r8\n\t\
			pop %rdi\n\t\
			pop %rsi\n\t\
			pop %rbp\n\t\
			pop %rbx\n\t\
			pop %rdx\n\t\
			pop %rcx\n\t\
			pop %rax\n\t\
			popfq" :::: "volatile"
		);
	}
}
