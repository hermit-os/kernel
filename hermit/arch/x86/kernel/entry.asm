
; Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
; All rights reserved.
;
; Redistribution and use in source and binary forms, with or without
; modification, are permitted provided that the following conditions are met:
;    * Redistributions of source code must retain the above copyright
;      notice, this list of conditions and the following disclaimer.
;    * Redistributions in binary form must reproduce the above copyright
;      notice, this list of conditions and the following disclaimer in the
;      documentation and/or other materials provided with the distribution.
;    * Neither the name of the University nor the names of its contributors
;      may be used to endorse or promote products derived from this software
;      without specific prior written permission.
;
; THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
; ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
; WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
; DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
; DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
; (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
; ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
; (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
; SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

; This is the kernel's entry point. We could either call main here,
; or we can use this to setup the stack or other nice stuff, like
; perhaps setting up the GDT and segments. Please note that interrupts
; are disabled at this point: More on interrupts later!

%include "config.inc"

[BITS 32]

extern kernel_start		; defined in linker script
extern kernel_end

; We use a special name to map this section at the begin of our kernel
; =>  Multiboot expects its magic number at the beginning of the kernel.
SECTION .mboot
global start
start:
    jmp stublet

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
    dd 0, 0, 0, 0,
    global base
    global limit
    base dq 0
    limit dq 0

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
    .Pointer:                    ; The GDT-pointer.
    dw $ - GDT64 - 1             ; Limit.
    dq GDT64                     ; Base.

SECTION .text
ALIGN 4
stublet:
    ; Initialize stack pointer
    mov esp, boot_stack
    add esp, KERNEL_STACK_SIZE - 16

    ; Interpret multiboot information
    mov DWORD [mb_info], ebx

    ; Initialize CPU features
    call cpu_init

    lgdt [GDT64.Pointer] ; Load the 64-bit global descriptor table.
    jmp GDT64.Code:start64 ; Set the code segment and enter 64-bit long mode.

; This will set up the x86 control registers:
; Caching and the floating point unit are enabled
; Bootstrap page tables are loaded and page size
; extensions (huge pages) enabled.
global cpu_init
cpu_init:

; initialize page tables

; map vga 1:1
%ifdef CONFIG_VGA
    push edi
    mov eax, VIDEO_MEM_ADDR   ; map vga
    and eax, 0xFFFFF000       ; page align lower half
    mov edi, eax
    shr edi, 9                ; (edi >> 12) * 8 (index for boot_pgt)
    add edi, boot_pgt
    or eax, 0x113             ; set present, global, writable and cache disable bits
    mov DWORD [edi], eax
    pop edi
%endif

    ; map multiboot info 1:1
    push edi
    mov eax, DWORD [mb_info]  ; map multiboot info
    cmp eax, 0
    je Lno_mbinfo
    and eax, 0xFFFFF000       ; page align lower half
    mov edi, eax
    shr edi, 9                ; (edi >> 12) * 8 (index for boot_pgt)
    add edi, boot_pgt
    or eax, 0x101             ; set present and global bits
    mov DWORD [edi], eax
Lno_mbinfo:
    pop edi

    ; map kernel 1:1, use a page size of 2MB
    push edi
    mov eax, kernel_start
    and eax, 0xFFE00000
    mov edi, eax
    shr edi, 18               ; (edi >> 21) * 8 (index for boot_pgd)
    add edi, boot_pgd
    or eax, 0x183
    mov DWORD [edi], eax
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


    ; we need to enable PAE modus
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    ; switch to the compatibility mode (which is part of long mode)
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    ; Set CR3
    mov eax, boot_pml4
    mov cr3, eax

    ; Set CR4
    mov eax, cr4
    and eax, 0xfffbf9ff     ; disable SSE
    or eax, (1 << 4)        ; enable PSE
    mov cr4, eax

    ; Set CR0
    mov eax, cr0
    and eax, ~(1 << 30)     ; enable caching
    and eax, ~(1 << 16)	    ; allow kernel write access to read-only pages
    or eax, (1 << 31)       ; enable paging
    or eax, (1 << 0)        ; long mode also needs PM-bit set
    mov cr0, eax

    ret

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
    mov ax, 0x00
    mov fs, ax
    mov gs, ax

    ; set default stack pointer
    mov rsp, boot_stack
    add rsp, KERNEL_STACK_SIZE-16

    ; jump to the boot processors's C code
    extern main
    call main
    jmp $

global gdt_flush
extern gp

; This will set up our new segment registers and is declared in
; C as 'extern void gdt_flush();'
gdt_flush:
    lgdt [gp]
    ret

; The first 32 interrupt service routines (ISR) entries correspond to exceptions.
; Some exceptions will push an error code onto the stack which is specific to
; the exception caused. To decrease the complexity, we handle this by pushing a
; Dummy error code of 0 onto the stack for any ISR that doesn't push an error
; code already.
;
; ISRs are registered as "Interrupt Gate".
; Therefore, the interrupt flag (IF) is already cleared.

; NASM macro which pushs also an pseudo error code
%macro isrstub_pseudo_error 1
    global isr%1
    isr%1:
        push byte 0 ; pseudo error code
        push byte %1
        jmp common_stub
%endmacro

; Similar to isrstub_pseudo_error, but without pushing
; a pseudo error code => The error code is already
; on the stack.
%macro isrstub 1
    global isr%1
    isr%1:
        push byte %1
        jmp common_stub
%endmacro

; Create isr entries, where the number after the
; pseudo error code represents following interrupts:
; 0: Divide By Zero Exception
; 1: Debug Exception
; 2: Non Maskable Interrupt Exception
; 3: Int 3 Exception
; 4: INTO Exception
; 5: Out of Bounds Exception
; 6: Invalid Opcode Exception
; 7: Coprocessor Not Available Exception
%assign i 0
%rep    8
    isrstub_pseudo_error i
%assign i i+1
%endrep

; 8: Double Fault Exception (With Error Code!)
isrstub 8

; 9: Coprocessor Segment Overrun Exception
isrstub_pseudo_error 9

; 10: Bad TSS Exception (With Error Code!)
; 11: Segment Not Present Exception (With Error Code!)
; 12: Stack Fault Exception (With Error Code!)
; 13: General Protection Fault Exception (With Error Code!)
; 14: Page Fault Exception (With Error Code!)
%assign i 10
%rep 5
    isrstub i
%assign i i+1
%endrep

; 15: Reserved Exception
; 16: Floating Point Exception
; 17: Alignment Check Exception
; 18: Machine Check Exceptio
; 19-31: Reserved
%assign i 15
%rep    17
    isrstub_pseudo_error i
%assign i i+1
%endrep

; NASM macro for asynchronous interrupts (no exceptions)
%macro irqstub 1
    global irq%1
    irq%1:
        push byte 0 ; pseudo error code
        push byte 32+%1
        jmp common_stub
%endmacro

; Create entries for the interrupts 0 to 23
%assign i 0
%rep    24
    irqstub i
%assign i i+1
%endrep

global apic_timer
apic_timer:
    push byte 0 ; pseudo error code
    push byte 123
    jmp common_stub

global apic_lint0
apic_lint0:
    push byte 0 ; pseudo error code
    push byte 124
    jmp common_stub

global apic_lint1
apic_lint1:
    push byte 0 ; pseudo error code
    push byte 125
    jmp common_stub

global apic_error
apic_error:
    push byte 0 ; pseudo error code
    push byte 126
    jmp common_stub

global apic_svr
apic_svr:
    push byte 0 ; pseudo error code
    push byte 127
    jmp common_stub

extern irq_handler
extern get_current_stack
extern finish_task_switch

global isrsyscall
; used to realize system calls
isrsyscall:
    cli
    ; set kernel stack
    extern get_kernel_stack
    call get_kernel_stack
    xchg rsp, rax ; => rax contains original rsp

    push rax ; contains original rsp
    push r15
    push r14
    push r13
    push r12
    push r11
    push rbp
    push rdx
    push rcx
    push rbx
    push rdi
    push rsi
    sti

    extern syscall_handler
    call syscall_handler

    cli
    pop rsi
    pop rdi
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    pop r10
    mov rsp, r10
    sti
    o64 sysret

global switch_context
ALIGN 8
switch_context:
    ; create on the stack a pseudo interrupt
    ; afterwards, we switch to the task with iret
    mov rax, rdi                ; rdi contains the address to store the old rsp
    pushf                       ; RFLAGS
    push QWORD 0x08             ; CS
    push QWORD rollback         ; RIP
    push QWORD 0x00             ; Interrupt number
    push QWORD 0x00edbabe       ; Error code
    push rax
    push rcx
    push rdx
    push rbx
    push rsp
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

    jmp common_switch

ALIGN 8
rollback:
    ret

ALIGN 8
common_stub:
    push rax
    push rcx
    push rdx
    push rbx
    push rsp
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

    ; use the same handler for interrupts and exceptions
    mov rdi, rsp
    call irq_handler

    cmp rax, 0
    je no_context_switch

common_switch:
    mov [rax], rsp             ; store old rsp
    call get_current_stack     ; get new rsp
    xchg rax, rsp

    ; set task switched flag
    mov rax, cr0
    or eax, 8
    mov cr0, rax

    ; set rsp0 in the task state segment
    extern set_kernel_stack
    call set_kernel_stack

    ; call cleanup code
    call finish_task_switch

no_context_switch:
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
    add rsp, 8
    pop rbx
    pop rdx
    pop rcx
    pop rax

    add rsp, 16
    iretq

SECTION .data

global mb_info:
ALIGN 8
mb_info:
    DQ 0

ALIGN 4096
global boot_stack
boot_stack:
    TIMES (KERNEL_STACK_SIZE) DB 0xcd

; Bootstrap page tables are used during the initialization.
ALIGN 4096
boot_pml4:
    DQ boot_pdpt + 0x107 ; PG_PRESENT | PG_GLOBAL | PG_RW | PG_USER
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x303 ; PG_PRESENT | PG_GLOBAL | PG_RW | PG_SELF (self-reference)
boot_pdpt:
    DQ boot_pgd + 0x107  ; PG_PRESENT | PG_GLOBAL | PG_RW | PG_USER
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x303 ; PG_PRESENT | PG_GLOBAL | PG_RW | PG_SELF (self-reference)
boot_pgd:
    DQ boot_pgt + 0x107  ; PG_PRESENT | PG_GLOBAL | PG_RW | PG_USER
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x303 ; PG_PRESENT | PG_GLOBAL | PG_RW | PG_SELF (self-reference)
boot_pgt:
    times 512 DQ 0

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
