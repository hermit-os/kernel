// Copyright (c) 2020 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

.section .text
.global _start
.global task_start
.extern pre_init
.extern task_entry

.align 16
_start:
	// initialize stack pointer
	mov $0x7ff0,%rax
	add 0x38(%rdi),%rax
	mov %rax, %rsp
	mov %rsp, %rbp

	call pre_init

l1:
	jmp l1

.align 16
task_start:
	mov %rdx, %rsp
	sti
	jmp task_entry
	jmp l1
