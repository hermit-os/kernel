// Copyright (c) 2020 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

.section .text
.global switch_to_task
.global switch_to_fpu_owner
.global Lpatch0
.global Lpatch1
.global Lpatch2
.global Lpatch3
.extern set_current_kernel_stack

.align 16
switch_to_task:
	// rdi = old_stack => the address to store the old rsp
	// rsi = new_stack => stack pointer of the new task

	pushfq
	push %rax
	push %rcx
	push %rdx
	push %rbx
	push %rbp
	push %rsi
	push %rdi
	push %r8
	push %r9
	push %r10
	push %r11
	push %r12
	push %r13
	push %r14
	push %r15
    // push fs registers
Lpatch0:
	jmp Lrdgs0  // we patch later this jump to enable rdfsbase/rdgsbase
	rdfsbaseq %rax
	push %rax
	jmp Lgo0
Lrdgs0:
	mov $0xc0000100, %ecx
	rdmsr
	sub $8, %rsp
	mov %edx, 4(%rsp)
	mov %eax, (%rsp)
Lgo0:
	// store the old stack pointer in the dereferenced first parameter\n\t\
	// and load the new stack pointer in the second parameter.\n\t\
	mov %rsp, (%rdi)
	mov %rsi, %rsp
	// Set task switched flag
	mov %cr0, %rax
	or $8, %rax
	mov %rax, %cr0
	// set stack pointer in TSS
	call set_current_kernel_stack
	// restore context
Lpatch1:
	jmp Lwrfsgs1    // we patch later this jump to enable wrfsbase/wrgsbase
	pop %rax
	wrfsbaseq %rax
	jmp Lgo1
Lwrfsgs1:
	mov $0xc0000100, %ecx
	mov 4(%rsp), %edx
	mov (%rsp), %eax
	add $8, %rsp
	wrmsr
Lgo1:
	pop %r15
	pop %r14
	pop %r13
	pop %r12
	pop %r11
	pop %r10
	pop %r9
	pop %r8
	pop %rdi
	pop %rsi
	pop %rbp
	pop %rbx
	pop %rdx
	pop %rcx
	pop %rax
	popfq
	ret

/// The function triggers a context switch to an idle task or
/// a task, which is alread owner of the FPU.
/// Consequently  the kernel don't set the task switched flag.
.align 16
switch_to_fpu_owner:
	// rdi = old_stack => the address to store the old rsp
	// rsi = new_stack => stack pointer of the new task

	// store context
	pushfq
	push %rax
	push %rcx
	push %rdx
	push %rbx
	push %rbp
	push %rsi
	push %rdi
	push %r8
	push %r9
	push %r10
	push %r11
	push %r12
	push %r13
	push %r14
	push %r15
	// push fs registers
Lpatch2:
	jmp Lrdgs2  // we patch later this jump to enable rdfsbase/rdgsbase
	rdfsbaseq %rax
	push %rax
	jmp Lgo2
Lrdgs2:
	mov $0xc0000100, %ecx
	rdmsr
	sub $8, %rsp
	mov %edx, 4(%rsp)
	mov %eax, (%rsp)
Lgo2:
	// store the old stack pointer in the dereferenced first parameter\n\t\
	// and load the new stack pointer in the second parameter.\n\t\
	mov %rsp, (%rdi)
	mov %rsi, %rsp
	// set stack pointer in TSS
	call set_current_kernel_stack
	// restore context
Lpatch3:
	jmp Lwrfsgs3    // we patch later this jump to enable wrfsbase/wrgsbase
	pop %rax
	wrfsbaseq %rax
	jmp Lgo3
Lwrfsgs3:
	mov $0xc0000100, %ecx
	mov 4(%rsp), %edx
	mov (%rsp), %eax
	add $8, %rsp
	wrmsr
Lgo3:
	pop %r15
	pop %r14
	pop %r13
	pop %r12
	pop %r11
	pop %r10
	pop %r9
	pop %r8
	pop %rdi
	pop %rsi
	pop %rbp
	pop %rbx
	pop %rdx
	pop %rcx
	pop %rax
	popfq
	ret
