; Copyright (c) 2010-2017 Stefan Lankes, RWTH Aachen University
;               2017 Colin Finck, RWTH Aachen University
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

; This is the kernel's entry point. We could either call main here,
; or we can use this to setup the stack or other nice stuff, like
; perhaps setting up the GDT and segments. Please note that interrupts
; are disabled at this point: More on interrupts later!

%include "hermit/config.asm"

[BITS 64]

extern kernel_start		; defined in linker script

; We use a special name to map this section at the begin of our kernel
; =>  Multiboot expects its magic number at the beginning of the kernel.
SECTION .mboot
global _start
_start:
    jmp start64

align 4
    global base
    global limit
    global cpu_freq
    global boot_processor
    global cpu_online
    global possible_cpus
    global current_boot_id
    global isle
    global possible_isles
    global phy_rcce_internals
    global phy_isle_locks
    global heap_phy_start_address
    global header_phy_start_address
    global heap_start_address
    global header_start_address
    global heap_size
    global header_size
    global disable_x2apic
    global mb_info
    global hbmem_base
    global hbmem_size
    global uhyve
    global image_size
    global uartport
    global cmdline
    global cmdsize
    global hcip
    global hcgateway
    global hcmask
    base dq 0
    limit dq 0
    cpu_freq dd 0
    boot_processor dd -1
    cpu_online dd 0
    possible_cpus dd 0
    phy_rcce_internals dq 0
    current_boot_id dd 0
    isle dd -1
    image_size dq 0
    phy_isle_locks dq 0
    heap_phy_start_address dq 0
    header_phy_start_address dq 0
    heap_size dd 0
    header_size dd 0
    possible_isles dd 1
    heap_start_address dq 0
    header_start_address dq 0
    disable_x2apic dd 1
    single_kernel dd 1
    mb_info dq 0
    hbmem_base dq 0
    hbmem_size dq 0
    uhyve dd 0
    uartport dq 0
    cmdline dq 0
    cmdsize dq 0
    hcip db  10,0,5,2
    hcgateway db 10,0,5,1
    hcmask db 255,255,255,0

; Bootstrap page tables are used during the initialization.
align 4096
boot_pml4:
    DQ boot_pdpt + 0x27  ; PG_PRESENT | PG_RW | PG_USER | PG_ACCESSED
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x223 ; PG_PRESENT | PG_RW | PG_ACCESSED | PG_SELF (self-reference)
boot_pdpt:
    DQ boot_pgd + 0x23   ; PG_PRESENT | PG_RW | PG_ACCESSED
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x223 ; PG_PRESENT | PG_RW | PG_ACCESSED | PG_SELF (self-reference)
boot_pgd:
    DQ boot_pgt + 0x23   ; PG_PRESENT | PG_RW | PG_ACCESSED
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x223 ; PG_PRESENT | PG_RW | PG_ACCESSED | PG_SELF (self-reference)
boot_pgt:
    times 512 DQ 0

SECTION .ktext
align 4
start64:
    ; reset registers to kill any stale realmode selectors
    mov eax, 0x10
    mov ds, eax
    mov ss, eax
    mov es, eax
    xor eax, eax
    mov fs, eax
    mov gs, eax

    ; clear DF flag => default value by entering a function
    ; => see ABI
    cld

    xor rax, rax
    mov eax, DWORD [cpu_online]
    cmp eax, 0
    jne Lno_pml4_init

    ; store pointer to the multiboot information
    mov [mb_info], QWORD rdx

	;
    ; relocate page tables
    mov rdi, boot_pml4
    mov rax, QWORD [rdi]
    sub rax, kernel_start
    add rax, [base]
    mov QWORD [rdi], rax

    mov rax, QWORD [rdi+511*8]
    sub rax, kernel_start
    add rax, [base]
    mov QWORD [rdi+511*8], rax

    mov rdi, boot_pdpt
    mov rax, QWORD [rdi]
    sub rax, kernel_start
    add rax, [base]
    mov QWORD [rdi], rax

    mov rax, QWORD [rdi+511*8]
    sub rax, kernel_start
    add rax, [base]
    mov QWORD [rdi+511*8], rax

    mov rdi, boot_pgd
    mov rax, QWORD [rdi]
    sub rax, kernel_start
    add rax, [base]
    mov QWORD [rdi], rax

    mov rax, QWORD [rdi+511*8]
    sub rax, kernel_start
    add rax, [base]
    mov QWORD [rdi+511*8], rax

    ; remap kernel
    mov rdi, kernel_start
    shr rdi, 18       ; (edi >> 21) * 8 (index for boot_pgd)
    add rdi, boot_pgd
    mov rax, [base]
    or rax, 0xA3      ; PG_GLOBAL isn't required because HermitCore is a single-address space OS
    xor rcx, rcx
    mov rsi, 510*0x200000
    sub rsi, kernel_start
    mov r11, QWORD [image_size]
Lremap:
    mov QWORD [rdi], rax
    add rax, 0x200000
    add rcx, 0x200000
    add rdi, 8
    ; note: the whole code segement has to fit in the first pgd
    cmp rcx, rsi
    jnl Lno_pml4_init
    cmp rcx, r11
    jl Lremap

Lno_pml4_init:
    ; Set CR3
    mov rax, boot_pml4
    sub rax, kernel_start
    add rax, [base]
    or rax, (1 << 0)          ; set present bit
    mov cr3, rax

%if MAX_CORES > 1
    mov eax, DWORD [cpu_online]
    cmp eax, 0
    jne Lsmp_main
%endif

    ; set default stack pointer
    mov rsp, boot_stack
    add rsp, KERNEL_STACK_SIZE-16
    xor rax, rax
    mov eax, [boot_processor]
    cmp eax, -1
    je L1
    imul eax, KERNEL_STACK_SIZE
    add rsp, rax
L1:
    mov rbp, rsp

    ; jump to the boot processors' entry point
    extern boot_processor_main
    call boot_processor_main
    jmp $

%if MAX_CORES > 1
ALIGN 64
Lsmp_main:
    xor rax, rax
    mov eax, DWORD [current_boot_id]

    ; set default stack pointer
    imul rax, KERNEL_STACK_SIZE
    add rax, boot_stack
    add rax, KERNEL_STACK_SIZE-16
    mov rsp, rax
    mov rbp, rsp

    extern application_processor_main
    call application_processor_main
    jmp $
%endif


SECTION .data

align 4096
global boot_stack
boot_stack:
    TIMES (MAX_CORES*KERNEL_STACK_SIZE) DB 0xcd

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
