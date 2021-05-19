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
	mov rax, 0x7ff0
	add rax, [rdi+0x38]
	mov rsp, rax
	mov rbp, rsp

	call pre_init

l1:
	jmp l1

.align 16
task_start:
	mov rsp, rdx
	sti
	jmp task_entry
	jmp l1
