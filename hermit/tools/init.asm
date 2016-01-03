; Copyright 2010-2015 Stefan Lankes, RWTH Aachen University
; All rights reserved.
;
; This software is licensed under the terms of the GNU General Public
; License version 2, as published by the Free Software Foundation, and
; may be copied, distributed, and modified under those terms.
;
; This program is distributed in the hope that it will be useful,
; but WITHOUT ANY WARRANTY; without even the implied warranty of
; MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
; GNU General Public License for more details.
;
; This is the kernel's entry point for the application processors.
; We switch to the protected mode and jump to HermitCore's kernel.

KERNEL_STACK_SIZE equ 0x100
kernel_start equ 0x800000

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

	mov esp, boot_stack+KERNEL_STACK_SIZE-16
	mov ebp, 0x00           ; dummy value
	jmp short stublet
	jmp $

; GDT for the protected mode
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

ALIGN 4
GDTR64:
    dw GDT64_end - GDT64 - 1     ; Limit.
    dq GDT64                     ; Base.

; we need a new GDT to switch in the 64bit modus
GDT64:                           ; Global Descriptor Table (64-bit).
    .Null: equ $ - GDT64         ; The null descriptor.
    dw 0                         ; Limit (low).
    dw 0                         ; Base (low).
    db 0                         ; Base (middle)
    db 0                         ; Access.
    db 0                         ; Granularity.
    db 0                         ; Base (high).
    .Code: equ $ - GDT64         ; The code descriptor.
    dw 0                         ; Limit (low).
    dw 0                         ; Base (low).
    db 0                         ; Base (middle)
    db 10011000b                 ; Access.
    db 00100000b                 ; Granularity.
    db 0                         ; Base (high).
    .Data: equ $ - GDT64         ; The data descriptor.
    dw 0                         ; Limit (low).
    dw 0                         ; Base (low).
    db 0                         ; Base (middle)
    db 10010010b                 ; Access.
    db 00000000b                 ; Granularity.
    db 0                         ; Base (high).
GDT64_end:

ALIGN 4
stublet:

; This will set up the x86 control registers:
; Caching and the floating point unit are enabled
; Bootstrap page tables are loaded and page size
; extensions (huge pages) enabled.
cpu_init:
; relocate page tables
    mov edi, boot_pml4
    add edi, ebp
    mov eax, DWORD [edi]
    add eax, ebp
    mov DWORD [edi], eax

    mov eax, DWORD [edi+511*8]
    add eax, ebp
    mov DWORD [edi+511*8], eax

    mov edi, boot_pdpt
    add edi, ebp
    mov eax, DWORD [edi]
    add eax, ebp
    mov DWORD [edi], eax

    mov eax, DWORD [edi+511*8]
    add eax, ebp
    mov DWORD [edi+511*8], eax

    mov edi, boot_pgd
    add edi, ebp
    mov eax, DWORD [edi]
    add eax, ebp
    mov DWORD [edi], eax

    mov eax, DWORD [edi+511*8]
    add eax, ebp
    mov DWORD [edi+511*8], eax

    ; initialize page tables

    ; map kernel at link address, use a page size of 2M
    mov eax, 0x00             ; lower part of the page addres, Linux will relocate this value
    and eax, 0xFFE00000
    or eax, 0x183
    mov ebx, 0x00             ; higher part of the page address, Linux will relocate this value
    mov edi, kernel_start
    and edi, 0xFFE00000
    shr edi, 18               ; (edi >> 21) * 8 (index for boot_pgd)
    add edi, boot_pgd
    add edi, ebp
    mov DWORD [edi], eax
    mov DWORD [edi+4], ebx

    ; check for long mode

    ; do we have the instruction cpuid?
    pushfd
    pop eax
    mov ecx, eax
    xor eax, 1 << 21
    push eax
    popfd
    pushfd
    pop eax
    push ecx
    popfd
    xor eax, ecx
    jz $ ; there is no long mode

    ; cpuid > 0x80000000?
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb $ ; It is less, there is no long mode.

    ; do we have a long mode?
    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29 ; Test if the LM-bit, which is bit 29, is set in the D-register.
    jz $ ; They aren't, there is no long mode.

    ; we need to enable PAE modus
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    ; Set CR3
    mov eax, boot_pml4
    add eax, ebp
    or eax, (1 << 0)        ; set present bit
    mov cr3, eax

    ; Set CR4 (PAE is already set)
    mov eax, cr4
    and eax, 0xfffbf9ff     ; disable SSE
    or eax, (1 << 7)        ; enable PGE
    or eax, (1 << 20)       ; enable SMEP
    mov cr4, eax

    ; Set CR0 (PM-bit is already set)
    mov eax, cr0
    and eax, ~(1 << 2)      ; disable FPU emulation
    or eax, (1 << 1)        ; enable FPU montitoring
    and eax, ~(1 << 30)     ; enable caching
    and eax, ~(1 << 29)     ; disable write through caching
    and eax, ~(1 << 16)     ; allow kernel write access to read-only pages
    or eax, (1 << 31)       ; enable paging
    mov cr0, eax

    lgdt [GDTR64]           ; Load the 64-bit global descriptor table.
    jmp GDT64.Code:start64  ; Set the code segment and enter 64-bit long mode.

[BITS 64]
ALIGN 8
start64:
    push kernel_start
    ret

ALIGN 16
global boot_stack
boot_stack:
    TIMES (KERNEL_STACK_SIZE) DB 0xcd

; Bootstrap page tables are used during the initialization.
ALIGN 4096
boot_pml4:
    DQ boot_pdpt + 0x7   ; PG_PRESENT | PG_RW | PG_USER
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x203 ; PG_PRESENT | PG_RW | PG_SELF (self-reference)
boot_pdpt:
    DQ boot_pgd + 0x7    ; PG_PRESENT | PG_RW | PG_USER
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x203 ; PG_PRESENT | PG_RW | PG_SELF (self-reference)
boot_pgd:
    DQ boot_pgt + 0x7    ; PG_PRESENT | PG_RW | PG_USER
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x203 ; PG_PRESENT | PG_RW | PG_SELF (self-reference)
boot_pgt:
%assign i 0
%rep    512
    DQ i*0x1000 + 0x103
%assign i i+1
%endrep
