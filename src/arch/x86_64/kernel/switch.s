// Copyright (c) 2020 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

.section .text
.global switch_to_task
.global switch_to_fpu_owner
.extern set_current_kernel_stack

.align 16
switch_to_task:
	// rdi = old_stack => the address to store the old rsp
	// rsi = new_stack => stack pointer of the new task

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
    // push fs registers
	mov ecx, 0xc0000100
	rdmsr
	sub rsp, 8
	mov [rsp+4], edx
	mov [rsp], eax
	// store the old stack pointer in the dereferenced first parameter\n\t\
	// and load the new stack pointer in the second parameter.\n\t\
	mov [rdi], rsp
	mov rsp, rsi
	// Set task switched flag
	mov rax, cr0
	or rax, 8
	mov cr0, rax
	// set stack pointer in TSS
	call set_current_kernel_stack
	// restore context
	mov ecx, 0xc0000100
	mov edx, [rsp+4]
	mov eax, [rsp]
	add rsp, 8
	wrmsr
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

/// The function triggers a context switch to an idle task or
/// a task, which is alread owner of the FPU.
/// Consequently  the kernel don't set the task switched flag.
.align 16
switch_to_fpu_owner:
	// rdi = old_stack => the address to store the old rsp
	// rsi = new_stack => stack pointer of the new task

	// store context
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
	// push fs registers
	mov ecx, 0xc0000100
	rdmsr
	sub rsp, 8
	mov [rsp+4], edx
	mov [rsp], eax
	// store the old stack pointer in the dereferenced first parameter\n\t\
	// and load the new stack pointer in the second parameter.\n\t\
	mov [rdi], rsp
	mov rsp, rsi
	// set stack pointer in TSS
	call set_current_kernel_stack
	// restore context
	mov ecx, 0xc0000100
	mov edx, [rsp+4]
	mov eax, [rsp]
	add rsp, 8
	wrmsr
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
