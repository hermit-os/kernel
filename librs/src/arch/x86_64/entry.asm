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


; This is the entry point called by a Multiboot-compliant loader for the
; boot processor, or by boot.asm for an application processor.


%include "hermit/config.asm"

MSR_EFER     equ 0xC0000080
EFER_NXE     equ (1 << 11)

[BITS 64]

extern kernel_start		; defined in linker script
extern boot_processor_main
extern application_processor_main
extern PERCORE

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
    global current_stack_address
    global current_percore_address
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
    current_stack_address dq boot_stack_bottom
    current_percore_address dq PERCORE

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

    ; set default stack pointer
    mov rsp, [current_stack_address]
    add rsp, (KERNEL_STACK_SIZE - 0x10)
    mov rbp, rsp

    ; Is this an Application Processor?
    xor rax, rax
    mov eax, DWORD [cpu_online]
    cmp eax, 0
    je boot_processor_init

    ; Then we're done and just call into the Application Processor Rust entry point.
    call application_processor_main
    jmp $

boot_processor_init:
    ; store pointer to the multiboot information
    mov [mb_info], QWORD rdx

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
    jnl Lremap_done
    cmp rcx, r11
    jl Lremap
Lremap_done:

    ; Set CR3
    mov rax, boot_pml4
    sub rax, kernel_start
    add rax, [base]
    mov cr3, rax

    ; Set EFER.NXE to enable early access to EXECUTE_DISABLE-protected memory.
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_NXE
    wrmsr

    ; Call into the Boot Processor Rust entry point.
    call boot_processor_main
    jmp $


SECTION .data

; This stack is used by the Boot Processor only.
align 4096
boot_stack_bottom:
    TIMES KERNEL_STACK_SIZE DB 0xcd
boot_stack_top:

; These page tables are used for bootstrapping and normal operation later.
align 4096
boot_pml4:
    DQ boot_pdpt + 0x3   ; PG_PRESENT | PG_RW
    times 510 DQ 0       ; PAGE_MAP_ENTRIES - 2
    DQ boot_pml4 + 0x3   ; PG_PRESENT | PG_RW
boot_pdpt:
    DQ boot_pgd + 0x3    ; PG_PRESENT | PG_RW
    times 511 DQ 0       ; PAGE_MAP_ENTRIES - 2
boot_pgd:
    DQ boot_pgt + 0x3    ; PG_PRESENT | PG_RW
    times 511 DQ 0       ; PAGE_MAP_ENTRIES - 2
boot_pgt:
    times 512 DQ 0

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
