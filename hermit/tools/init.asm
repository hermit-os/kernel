; 
; Copyright 2010-2015 Stefan Lankes, RWTH Aachen University
;
; This is the kernel's entry point for the application processors.
; We switch to the protected mode and jump to HermitCore's kernel.
;
; This source code is licensed under the GNU General Public License,
; Version 2.  See the file COPYING for more details.
;

[BITS 16]
SECTION .text
GLOBAL _start
ORG 0x00
_start:
	cli
	lgdt [gdtr]

	; switch to protected mode by setting PE bit
	mov eax, cr0
	or al, 0x1
	mov cr0, eax

	; far jump to the 32bit code
	jmp dword codesel : _pmstart

[BITS 32]
ALIGN 4
_pmstart:
	xor eax, eax
	mov ax, datasel
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax
	mov ss, ax

	xor ebx, ebx ; invalid multiboot address
	mov esp, -16
	push DWORD 0x00 ; dummy value
	push DWORD 0x00 ; dummy value
	jmp codesel : codeaddr
	jmp $

ALIGN 4
gdtr:                           ; descritor table
        dw gdt_end-gdt-1        ; limit
        dd gdt                  ; base adresse
gdt:
        dd 0,0                  ; null descriptor
codesel equ $-gdt
        dw 0xFFFF               ; segment size 0..15
        dw 0x0000               ; segment address 0..15
        db 0x00                 ; segment address 16..23
        db 0x9A                 ; access permissions und type
        db 0xCF                 ; additional information and segment size 16...19
        db 0x00                 ; segment address 24..31
datasel equ $-gdt
        dw 0xFFFF               ; segment size 0..15
        dw 0x0000               ; segment address 0..15
        db 0x00                 ; segment address 16..23
        db 0x92                 ; access permissions and type
        db 0xCF                 ; additional informationen and degment size 16...19
        db 0x00                 ; segment address 24..31
gdt_end:

codeaddr equ 0x1400000		; address of the 32bit kernel
