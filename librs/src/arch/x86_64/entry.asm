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
    global single_kernel
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
    global host_logical_addr
    global current_stack_address
    global current_percore_address
    global boot_gtod
    base dq 0                                       ; 0x08
    limit dq 0                                      ; 0x10
    cpu_freq dd 0                                   ; 0x18
    boot_processor dd -1                            ; 0x1c  UNUSED
    cpu_online dd 0                                 ; 0x20
    possible_cpus dd 0                              ; 0x24  UNUSED
    phy_rcce_internals dq 0                         ; 0x28  UNUSED
    current_boot_id dd 0                            ; 0x30
    isle dd -1                                      ; 0x34  UNUSED
    image_size dq 0                                 ; 0x38
    phy_isle_locks dq 0                             ; 0x40  UNUSED
    heap_phy_start_address dq 0                     ; 0x48  UNUSED
    header_phy_start_address dq 0                   ; 0x50  UNUSED
    heap_size dd 0                                  ; 0x58  UNUSED
    header_size dd 0                                ; 0x5c  UNUSED
    possible_isles dd 1                             ; 0x60  UNUSED
    heap_start_address dq 0                         ; 0x64  UNUSED
    header_start_address dq 0                       ; 0x6c  UNUSED
    disable_x2apic dd 1                             ; 0x74  UNUSED
    single_kernel dd 1                              ; 0x78
    mb_info dq 0                                    ; 0x7c
    hbmem_base dq 0                                 ; 0x84  UNUSED
    hbmem_size dq 0                                 ; 0x8c  UNUSED
    uhyve dd 0                                      ; 0x94
    uartport dq 0                                   ; 0x98
    cmdline dq 0                                    ; 0xa0
    cmdsize dq 0                                    ; 0xa8
    hcip db  10,0,5,2                               ; 0xb0, 0xb1, 0xb2, 0xb3
    hcgateway db 10,0,5,1                           ; 0xb4, 0xb5, 0xb6, 0xb7
    hcmask db 255,255,255,0                         ; 0xb8, 0xb9, 0xba, 0xbb
    host_logical_addr dq 0                          ; 0xbc  UNUSED
    current_stack_address dq boot_stack_bottom      ; 0xc4
    current_percore_address dq PERCORE              ; 0xcc
    boot_gtod dq 0                                  ; 0xd4

; These page tables are used for bootstrapping and normal operation later.
; They must be located at (entry point + 4096) for uhyve.
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
    mov rsp, QWORD [current_stack_address]
    add rsp, (KERNEL_STACK_SIZE - 0x10)
    mov rbp, rsp

    ; Assume and then check whether this is an Application Processor.
    ; Skip boot processor initialization steps in this case.
    mov r8, application_processor_main
    mov eax, DWORD [cpu_online]
    cmp eax, 0
    jne boot_processor_init_done

boot_processor_init:
    ; Overwrite the entry point to call.
    mov r8, boot_processor_main

    ; store pointer to the multiboot information
    mov [mb_info], QWORD rdx

    ; Set EFER.NXE to enable early access to EXECUTE_DISABLE-protected memory.
    ; For the Application Processor, this has already been done in boot.asm or by uhyve.
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_NXE
    wrmsr

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
    jnl boot_processor_init_done
    cmp rcx, r11
    jl Lremap
boot_processor_init_done:

    ; Set CR3
    ; If this is an Application Processor and we're running bare-metal/QEMU, this has already been done in boot.asm.
    ; However, this step is required when running on uhyve.
    mov rax, boot_pml4
    sub rax, kernel_start
    add rax, [base]
    mov cr3, rax

    ; Call into the desired Rust entry point.
    call r8
    jmp $

; NASM macro for asynchronous interrupts (no exceptions)
%macro irqstub 1
global irq%1
align 8
irq%1:
    push rax
    push rcx
    push rdx
    push rbx
    push QWORD [rsp+9*8]        ; push user-space rsp, which is already on the stack
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
    mov rdi, %1
    extern unhandled_interrupt
    call unhandled_interrupt
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
    iretq
%endmacro

; Create entries for the interrupts 0 to 31
%assign i 0
%rep    32
    irqstub i
%assign i i+1
%endrep


SECTION .data

; This stack is used by the Boot Processor only.
align 4096
boot_stack_bottom:
    TIMES KERNEL_STACK_SIZE DB 0xcd

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
