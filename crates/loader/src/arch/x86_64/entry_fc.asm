; This is the kernel's entry point, if RustyHermit is running with
; FireCracker. FireCracker assumes a 64 bit Linux kernel.

[BITS 64]

%define BOOT_STACK_SIZE 4096

extern kernel_start		; defined in linker script
extern kernel_end

; Move entry point at the beginning of the elf file
SECTION .mboot
ALIGN 8
global _start
_start:
    cli ; avoid any interrupt

    ; Initialize stack pointer
    mov rsp, boot_stack
    add rsp, BOOT_STACK_SIZE - 16

    mov QWORD [boot_params], rsi

    ; initialize page tables
    ; map kernel 1:1
    push rdi
    push rbx
    push rcx
    mov rcx, kernel_start
    mov rbx, kernel_end
    add rbx, 0x1000
 L0: cmp rcx, rbx
    jae L1
    mov rax, rcx
    and eax, 0xFFFFF000       ; page align lower half
    mov rdi, rax
    shr rdi, 9                ; (edi >> 12) * 8 (index for boot_pgt)
    add rdi, boot_pgt1
    or rax, 0x3               ; set present and writable bits
    mov QWORD [rdi], rax
    add rcx, 0x1000
    jmp L0
 L1:
    pop rcx
    pop rbx
    pop rdi

    ; Set CR3
    mov eax, boot_pml4
    ;or eax, (1 << 0)        ; set present bit
    mov cr3, rax

    ; we need to enable PAE modus
    mov rax, cr4
    or eax, 1 << 5
    mov cr4, rax

    ; switch to the compatibility mode (which is part of long mode)
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr


    ; Set CR4
    mov rax, cr4
    and eax, 0x00000000fffbf9ff     ; disable SSE
    ;or eax, (1 << 7)       ; enable PGE
    mov cr4, rax
     
    ; Set CR0 (PM-bit is already set)
    mov rax, cr0
    and rax, ~(1 << 2)      ; disable FPU emulation
    or eax, (1 << 1)        ; enable FPU montitoring
    and rax, ~(1 << 30)     ; enable caching
    and rax, ~(1 << 29)     ; disable write through caching
    and rax, ~(1 << 16)	    ; allow kernel write access to read-only pages
    or eax, (1 << 31)       ; enable paging
    mov cr0, rax


    lgdt [GDT64.Pointer] ; Load the 64-bit global descriptor table.
    jmp start64 ; Set the code segment and enter 64-bit long mode.

SECTION .text
ALIGN 8
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

global boot_params:
ALIGN 8
boot_params:
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