/* Copyright (c) 2017-2018 Stefan Lankes, RWTH Aachen University
 *
 * MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining
 * a copy of this software and associated documentation files (the
 * "Software"), to deal in the Software without restriction, including
 * without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to
 * permit persons to whom the Software is furnished to do so, subject to
 * the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
 * LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
 * WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 */

#[inline(never)]
#[naked]
pub extern "C" fn switch(old_stack: *mut usize, new_stack: usize) {
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
			popfq" :: "r{rdi}"(old_stack as u64), "r{rsi}"(new_stack as u64):: "volatile"
		);
	}
}
