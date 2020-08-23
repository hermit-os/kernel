
; Copyright (c) 2010-2016, Stefan Lankes, RWTH Aachen University
; All rights reserved.
;
;
; Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
; http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
; http://opensource.org/licenses/MIT>, at your option. This file may not be
; copied, modified, or distributed except according to those terms.

; This is the kernel's entry point. We could either call main here,
; or we can use this to setup the stack or other nice stuff, like
; perhaps setting up the GDT and segments. Please note that interrupts
; are disabled at this point: More on interrupts later!

[BITS 32]

%define BOOT_STACK_SIZE 4096

extern kernel_start		; defined in linker script
extern kernel_end

; We use a special name to map this section at the begin of our kernel
; =>  Multiboot expects its magic number at the beginning of the kernel.
SECTION .mboot

; This part MUST be 4 byte aligned, so we solve that issue using 'ALIGN 4'.
ALIGN 4
mboot:
    ; Multiboot macros to make a few lines more readable later
    MULTIBOOT_PAGE_ALIGN	equ (1 << 0)
    MULTIBOOT_MEMORY_INFO	equ (1 << 1)
    MULTIBOOT_HEADER_MAGIC	equ 0x1BADB002
    MULTIBOOT_HEADER_FLAGS	equ MULTIBOOT_PAGE_ALIGN | MULTIBOOT_MEMORY_INFO
    MULTIBOOT_CHECKSUM		equ -(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_FLAGS)

    ; This is the GRUB Multiboot header. A boot signature
    dd MULTIBOOT_HEADER_MAGIC
    dd MULTIBOOT_HEADER_FLAGS
    dd MULTIBOOT_CHECKSUM
    dd 0, 0, 0, 0, 0 ; address fields

ALIGN 4
; we need already a valid GDT to switch in the 64bit modus
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
    db 10011010b                 ; Access.
    db 00100000b                 ; Granularity.
    db 0                         ; Base (high).
    .Data: equ $ - GDT64         ; The data descriptor.
    dw 0                         ; Limit (low).
    dw 0                         ; Base (low).
    db 0                         ; Base (middle)
    db 10010010b                 ; Access.
    db 00000000b                 ; Granularity.
    db 0                         ; Base (high).
    .Pointer:                    ; The GDT-pointer.
    dw $ - GDT64 - 1             ; Limit.
    dq GDT64                     ; Base.

SECTION .text
ALIGN 4
global _start
_start:
    cli ; avoid any interrupt

    ; Initialize stack pointer
    mov esp, boot_stack
    add esp, BOOT_STACK_SIZE - 16

    ; Interpret multiboot information
    mov DWORD [mb_info], ebx

; This will set up the x86 control registers:
; Caching and the floating point unit are enabled
; Bootstrap page tables are loaded and page size
; extensions (huge pages) enabled.
cpu_init:

    ; initialize page tables
    ; map kernel 1:1
    push edi
    push ebx
    push ecx
    mov ecx, kernel_start
    mov ebx, kernel_end
    add ebx, 0x1000
L0: cmp ecx, ebx
    jae L1
    mov eax, ecx
    and eax, 0xFFFFF000       ; page align lower half
    mov edi, eax
    shr edi, 9                ; (edi >> 12) * 8 (index for boot_pgt)
    add edi, boot_pgt1
    or eax, 0x3               ; set present and writable bits
    mov DWORD [edi], eax
    add ecx, 0x1000
    jmp L0
L1:
    pop ecx
    pop ebx
    pop edi

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
    jz Linvalid

    ; cpuid > 0x80000000?
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb Linvalid ; It is less, there is no long mode.

    ; do we have a long mode?
    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29 ; Test if the LM-bit, which is bit 29, is set in the D-register.
    jz Linvalid ; They aren't, there is no long mode.

    ; Set CR3
    mov eax, boot_pml4
    ;or eax, (1 << 0)        ; set present bit
    mov cr3, eax

    ; we need to enable PAE modus
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    ; switch to the compatibility mode (which is part of long mode)
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    ; Set CR4
    mov eax, cr4
    and eax, 0xfffbf9ff     ; disable SSE
    ;or eax, (1 << 7)       ; enable PGE
    mov cr4, eax

    ; Set CR0 (PM-bit is already set)
    mov eax, cr0
    and eax, ~(1 << 2)      ; disable FPU emulation
    or eax, (1 << 1)        ; enable FPU montitoring
    and eax, ~(1 << 30)     ; enable caching
    and eax, ~(1 << 29)     ; disable write through caching
    and eax, ~(1 << 16)	    ; allow kernel write access to read-only pages
    or eax, (1 << 31)       ; enable paging
    mov cr0, eax

    lgdt [GDT64.Pointer] ; Load the 64-bit global descriptor table.
    jmp GDT64.Code:start64 ; Set the code segment and enter 64-bit long mode.

; there is no long mode
Linvalid:
    jmp $


[BITS 64]
start64:
    ; initialize segment registers
    mov ax, GDT64.Data
    mov ds, ax
    mov es, ax
    mov ss, ax
    xor ax, ax
    mov fs, ax
    mov gs, ax
    cld
    ; set default stack pointer
    mov rsp, boot_stack
    add rsp, BOOT_STACK_SIZE-16

    ; jump to the boot processors's C code
    extern loader_main
    jmp loader_main
    jmp $

SECTION .data

global mb_info:
ALIGN 8
mb_info:
    DQ 0

ALIGN 4096
global boot_stack
boot_stack:
    TIMES (BOOT_STACK_SIZE) DB 0xcd

; Bootstrap page tables are used during the initialization.
ALIGN 4096
boot_pml4:
    DQ boot_pdpt + 0x3  ; PG_PRESENT | PG_RW
    times 510 DQ 0      ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x3  ; PG_PRESENT | PG_RW
boot_pdpt:
    DQ boot_pgd + 0x3   ; PG_PRESENT | PG_RW
    times 511 DQ 0      ; PAGE_MAP_ENTRIES - 1
boot_pgd:
    DQ boot_pgt1 + 0x3  ; PG_PRESENT | PG_RW
    DQ boot_pgt2 + 0x3  ; PG_PRESENT | PG_RW
    times 510 DQ 0      ; PAGE_MAP_ENTRIES - 1
boot_pgt1:
    times 512 DQ 0
boot_pgt2:
    times 512 DQ 0

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
