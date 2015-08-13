
; Copyright (c) 2010-2015, Stefan Lankes, RWTH Aachen University
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

[BITS 64]

extern kernel_start		; defined in linker script
extern kernel_end

MSR_FS_BASE equ 0xc0000100
MSR_GS_BASE equ 0xc0000101

; We use a special name to map this section at the begin of our kernel
; =>  Multiboot expects its magic number at the beginning of the kernel.
SECTION .mboot
global start
start:
    jmp start64

align 4
    global base
    global limit
    global cpu_freq
    global boot_processor
    global cpu_online
    global possible_cpus
    global timer_ticks
    global current_boot_id
    base dq 0
    limit dq 0
    cpu_freq dd 0
    boot_processor dd -1
    cpu_online dd 0
    possible_cpus dd 0
    timer_ticks dq 0
    current_boot_id dd 0

SECTION .text
align 4
start64:
    ; initialize segment registers
    mov ax, 0x00
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov ax, 0x00

    mov eax, DWORD [cpu_online]
    cmp eax, 0
    jne Lno_pml4_init

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
    shr rdi, 18               ; (edi >> 21) * 8 (index for boot_pgd)
    add rdi, boot_pgd
    mov rax, [base]
    or rax, 0x183
    mov QWORD [rdi], rax

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

    ; jump to the boot processors's C code
    extern main
    call main
    jmp $

%if MAX_CORES > 1
Lsmp_main:
    xor rax, rax
    mov eax, DWORD [current_boot_id]

    ; set default stack pointer
    imul rax, KERNEL_STACK_SIZE
    add rax, boot_stack
    add rax, KERNEL_STACK_SIZE-16
    mov rsp, rax
    mov rbp, rsp

    extern smp_start
    call smp_start
    jmp $
%endif

ALIGN 4
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
    align 16
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
    align 16
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
    align 16
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
align 16
apic_timer:
    push byte 0 ; pseudo error code
    push byte 123
    jmp common_stub

global apic_lint0
align 16
apic_lint0:
    push byte 0 ; pseudo error code
    push byte 124
    jmp common_stub

global apic_lint1
align 16
apic_lint1:
    push byte 0 ; pseudo error code
    push byte 125
    jmp common_stub

global apic_error
align 16
apic_error:
    push byte 0 ; pseudo error code
    push byte 126
    jmp common_stub

global apic_svr
align 16
apic_svr:
    push byte 0 ; pseudo error code
    push byte 127
    jmp common_stub

extern irq_handler
extern get_current_stack
extern finish_task_switch
extern syscall_handler
extern kernel_stack

global isrsyscall
align 16
; used to realize system calls
isrsyscall:
    ; IF flag is already cleared
    ; cli
    ; only called from user space => get kernel-level selector
    swapgs
    ; get kernel stack
    xchg rsp, [gs:kernel_stack]

    ; push old rsp and restore [gs:kernel_stack]
    push QWORD [gs:kernel_stack]
    mov QWORD [gs:kernel_stack], rsp

    ; save registers accross function call
    push r8
    push r9
    push r10
    push r11
    push rdx
    push rcx
    push rdi
    push rsi

    ; push system call number
    ; push rax

    ; syscall stores in rcx the return address
    ; => using of r10 for the temporary storage of the 4th argument
    mov rcx, r10

    ; during a system call, HermitCore allows interrupts
    sti

    extern syscall_table
    call [rax*8+syscall_table]
    push rax ; result, which we have to return

    extern check_ticks
    call check_ticks

    extern check_timers
    call check_timers

    extern check_scheduling
    call check_scheduling

    cli

    ; restore registers
    pop rax
    pop rsi
    pop rdi
    pop rcx
    pop rdx
    pop r11
    pop r10
    pop r9
    pop r8

    ; restore user-level stack
    mov rsp, [rsp]

    ; set user-level selector
    swapgs
    ; EFLAGS (and IF flag) will be restored by sysret
    ; sti
    o64 sysret

global switch_context
align 16
switch_context:
    ; create on the stack a pseudo interrupt
    ; afterwards, we switch to the task with iret
    push QWORD 0x10             ; SS
    push rsp                    ; RSP
    add QWORD [rsp], 0x08       ; => value of rsp before the creation of a pseudo interrupt
    push QWORD 0x1202           ; RFLAGS
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
    ; push fs and gs registers
global Lpatch0
Lpatch0:
    jmp short Lrdfsgs1  ; we patch later this jump to enable rdfsbase/rdgsbase
    rdfsbase rax
    rdgsbase rdx
    push rax
    push rdx
    jmp short Lgo1
Lrdfsgs1:
    mov ecx, MSR_FS_BASE
    rdmsr
    sub rsp, 8
    mov DWORD [rsp+4], edx
    mov DWORD [rsp], eax
    mov ecx, MSR_GS_BASE
    rdmsr
    sub rsp, 8
    mov DWORD [rsp+4], edx
    mov DWORD [rsp], eax
Lgo1:

    mov rax, rdi		; rdi contains the address to store the old rsp

    jmp common_switch

align 16
rollback:
    ret

align 16
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
    ; push fs and gs registers
global Lpatch1
Lpatch1: 
    jmp short Lrdfsgs2  ; we patch later this jump to enable rdfsbase/rdgsbase
    rdfsbase rax
    rdgsbase rdx
    push rax
    push rdx
    jmp short Lgo2
Lrdfsgs2:
    mov ecx, MSR_FS_BASE
    rdmsr
    sub rsp, 8
    mov DWORD [rsp+4], edx
    mov DWORD [rsp], eax
    mov ecx, MSR_GS_BASE
    rdmsr
    sub rsp, 8
    mov DWORD [rsp+4], edx
    mov DWORD [rsp], eax
Lgo2:
    ; do we interrupt user-level code?
    cmp QWORD [rsp+24+18*8], 0x08
    je short kernel_space1
    swapgs  ; set GS to the kernel selector
kernel_space1:

    ; use the same handler for interrupts and exceptions
    mov rdi, rsp
    call irq_handler

    cmp rax, 0
    je no_context_switch

common_switch:
    mov QWORD [rax], rsp       ; store old rsp
    call get_current_stack     ; get new rsp
    mov rsp, rax

    ; set task switched flag
    mov rax, cr0
    or rax, 8
    mov cr0, rax

    ; call cleanup code
    call finish_task_switch

no_context_switch:
    ; restore fs / gs register
global Lpatch2
Lpatch2:
    jmp short Lwrfsgs    ; we patch later this jump to enable wrfsbase/wrgsbase
    add rsp, 8
    ;pop r15
    ;wrgsbase r15
    pop r15
    wrfsbase r15
    jmp short Lgo3
Lwrfsgs:
    ;mov ecx, MSR_GS_BASE
    ;mov edx, DWORD [rsp+4]
    ;mov eax, DWORD [rsp]
    add rsp, 8
    ;wrmsr
    add rsp, 8 ; ignore gs register
    mov ecx, MSR_FS_BASE
    mov edx, DWORD [rsp+4]
    mov eax, DWORD [rsp]
    add rsp, 8
    wrmsr
Lgo3:
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

    ; do we interrupt user-level code?
    cmp QWORD [rsp+24], 0x08
    je short kernel_space2
    swapgs  ; set GS to the user-level selector
kernel_space2:
    add rsp, 16
    iretq

SECTION .data

align 4096
global boot_stack
boot_stack:
    TIMES (MAX_CORES*KERNEL_STACK_SIZE) DB 0xcd

; Bootstrap page tables are used during the initialization.
align 4096
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
    times 512 DQ 0

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
