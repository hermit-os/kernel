; Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
;               2018 Colin Finck, RWTH Aachen University
;
; MIT License
;
; Permission is hereby granted, free of charge, to any person obtaining
; a copy of this software and associated documentation files (the
; "Software"), to deal in the Software without restriction, including
; without limitation the rights to use, copy, modify, merge, publish,
; distribute, sublicense, and/or sell copies of the Software, and to
; permit persons to whom the Software is furnished to do so, subject to
; the following conditions:
;
; The above copyright notice and this permission notice shall be
; included in all copies or substantial portions of the Software.
;
; THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
; EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
; MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
; NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
; LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
; OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
; WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

extern set_current_kernel_stack

section .ktext
bits 64

MSR_FS_BASE equ 0xc0000100

global switch
align 8
switch:
	; rdi => the address to store the old rsp
	; rsi => stack pointer of the new task

	; save context
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

	; push the fs register used for Thread-Local Storage
	mov ecx, MSR_FS_BASE
	rdmsr
	sub rsp, 8
	mov DWORD [rsp+4], edx
	mov DWORD [rsp], eax

	; store the old stack pointer in the dereferenced first parameter
	; and load the new stack pointer in the second parameter.
	mov QWORD [rdi], rsp
	mov rsp, rsi

	; Set task switched flag
	mov rax, cr0
	or rax, 8
	mov cr0, rax

	; set stack pointer in TSS
	call set_current_kernel_stack

	; restore the fs register
	mov ecx, MSR_FS_BASE
	mov edx, DWORD [rsp+4]
	mov eax, DWORD [rsp]
	wrmsr
	add esp, 8

	; restore remaining context
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
