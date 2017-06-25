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

%include "hermit/config.asm"

[BITS 64]

extern kernel_start		; defined in linker script

MSR_FS_BASE equ 0xc0000100
MSR_GS_BASE equ 0xc0000101
MSR_KERNEL_GS_BASE equ 0xc0000102

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
    cmdline	dq 0
    cmdsize	dq 0

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
    xor eax, eax
    mov ds, eax
    mov ss, eax
    mov es, eax
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

    ; map multiboot info
    mov rax, QWORD [mb_info]
    and rax, ~0xFFF           ; page align lower half
    cmp rax, 0
    je Lno_mbinfo
    mov rdi, rax
    shr rdi, 9                ; (edi >> 12) * 8 (index for boot_pgt)
    add rdi, boot_pgt
    or rax, 0x23              ; set present, accessed and writable bits
    mov QWORD [rdi], rax
Lno_mbinfo:
    ; remap kernel
    mov rdi, kernel_start
    shr rdi, 18       ; (edi >> 21) * 8 (index for boot_pgd)
    add rdi, boot_pgd
    mov rax, [base]
    or rax, 0xA3      ; PG_GLOBAL isn't required because HermitCore is a single-address space OS
    xor rcx, rcx
    mov rsi, 510*0x200000
    sub rsi, kernel_start
Lremap:
    mov QWORD [rdi], rax
    add rax, 0x200000
    add rcx, 0x200000
    add rdi, 8
    ; note: the whole code segement muust fit in the first pgd
    cmp rcx, rsi
    jnb Lno_pml4_init
    cmp rcx, QWORD [image_size]
    jb Lremap

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
    extern hermit_main
    call hermit_main
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

    extern smp_start
    call smp_start
    jmp $
%endif

ALIGN 64
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
    align 64
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
    align 64
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
; 18: Machine Check Exception
; 19-31: Reserved
%assign i 15
%rep    17
    isrstub_pseudo_error i
%assign i i+1
%endrep

; NASM macro for asynchronous interrupts (no exceptions)
%macro irqstub 1
    global irq%1
    align 64
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

; Create entries for the interrupts 80 to 82
%assign i 80
%rep 3
  irqstub i
%assign i i+1
%endrep

global wakeup
align 64
wakeup:
    push byte 0 ; pseudo error code
	push byte 121
	jmp common_stub

global mmnif_irq
align 64
mmnif_irq:
    push byte 0 ; pseudo error code
	push byte 122
	jmp common_stub

global apic_timer
align 64
apic_timer:
    push byte 0 ; pseudo error code
    push byte 123
    jmp common_stub

global apic_lint0
align 64
apic_lint0:
    push byte 0 ; pseudo error code
    push byte 124
    jmp common_stub

global apic_lint1
align 64
apic_lint1:
    push byte 0 ; pseudo error code
    push byte 125
    jmp common_stub

global apic_error
align 64
apic_error:
    push byte 0 ; pseudo error code
    push byte 126
    jmp common_stub

global apic_svr
align 64
apic_svr:
    push byte 0 ; pseudo error code
    push byte 127
    jmp common_stub

extern irq_handler
extern get_current_stack
extern finish_task_switch
extern syscall_handler
extern kernel_stack

global getcontext
align 64
getcontext:
    cli
    ; save general purpose regsiters
    mov QWORD [rdi + 0x00], r15
    mov QWORD [rdi + 0x08], r14
    mov QWORD [rdi + 0x10], r13
    mov QWORD [rdi + 0x18], r12
    mov QWORD [rdi + 0x20], r9
    mov QWORD [rdi + 0x28], r8
    mov QWORD [rdi + 0x30], rdi
    mov QWORD [rdi + 0x38], rsi
    mov QWORD [rdi + 0x40], rbp
    mov QWORD [rdi + 0x48], rbx
    mov QWORD [rdi + 0x50], rdx
    mov QWORD [rdi + 0x58], rcx
    lea rax, [rsp + 0x08]
    mov QWORD [rdi + 0x60], rax
    mov rax, QWORD [rsp]
    mov QWORD [rdi + 0x68], rax
    ; save FPU state
    fnstenv [rdi + 0x74]
    lea rax, [rdi + 0x70]
    stmxcsr [rax]
    xor rax, rax
    sti
    ret

global setcontext
align 64
setcontext:
    cli
    ; restore FPU state
    fldenv [rdi + 0x74]
    lea rax, [rdi + 0x70]
    ldmxcsr [rax]
    ; restore general purpose registers
    mov r15, QWORD [rdi + 0x00]
    mov r14, QWORD [rdi + 0x08]
    mov r13, QWORD [rdi + 0x10]
    mov r12, QWORD [rdi + 0x18]
    mov  r9, QWORD [rdi + 0x20]
    mov  r8, QWORD [rdi + 0x28]
    mov rdi, QWORD [rdi + 0x30]
    mov rsi, QWORD [rdi + 0x38]
    mov rbp, QWORD [rdi + 0x40]
    mov rbx, QWORD [rdi + 0x48]
    mov rdx, QWORD [rdi + 0x50]
    mov rcx, QWORD [rdi + 0x58]
    mov rsp, QWORD [rdi + 0x60]
    push QWORD [rdi + 0x68]
    xor rax, rax
    sti
    ret

global __startcontext
align 64
__startcontext:
    mov rsp, rbx
    pop rdi
    cmp rdi, 0
    je Lno_context

    call setcontext

Lno_context:
    extern exit
    call exit
    jmp $

global switch_context
align 64
switch_context:
    ; by entering a function the DF flag has to be cleared => see ABI
    cld
    ; create on the stack a pseudo interrupt
    ; afterwards, we switch to the task with iret
    push QWORD 0x10             ; SS
    push rsp                    ; RSP
    add QWORD [rsp], 0x08       ; => value of rsp before the creation of a pseudo interrupt
    pushfq                      ; RFLAGS
    push QWORD 0x08             ; CS
    push QWORD rollback         ; RIP
    push QWORD 0x00edbabe       ; Error code
    push QWORD 0x00             ; Interrupt number
    push rax
    push rcx
    push rdx
    push rbx
    push QWORD [rsp+9*8]
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

align 64
rollback:
    ret

align 64
common_stub:
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

%ifidn SAVE_FPU,ON
    ; set task switched flag
    mov rax, cr0
    or rax, 8
    mov cr0, rax
%endif

    ; call cleanup code
    call finish_task_switch

no_context_switch:
    ; do we interrupt user-level code?
    cmp QWORD [rsp+24+18*8], 0x08
    je short kernel_space2
    swapgs  ; set GS to the user-level selector
kernel_space2:
    ; restore fs / gs register
global Lpatch2
Lpatch2:
    jmp short Lwrfsgs    ; we patch later this jump to enable wrfsbase/wrgsbase
    pop r15
    ;wrgsbase r15        ; currently, we don't use the gs register
    pop r15
    wrfsbase r15
    jmp short Lgo3
Lwrfsgs:
    ;mov ecx, MSR_GS_BASE
    ;mov edx, DWORD [rsp+4]
    ;mov eax, DWORD [rsp]
    add rsp, 8
    ;wrmsr               ; currently, we don't use the gs register
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

    add rsp, 16
    iretq

global is_uhyve
align 64
is_uhyve:
   mov eax, DWORD [uhyve]
   ret

global is_single_kernel
align 64
is_single_kernel:
    mov eax, DWORD [single_kernel]
    ret


global sighandler_epilog
sighandler_epilog:
    ; restore only those registers that might have changed between returning
	; from IRQ and execution of signal handler
	add rsp, 2 * 8		; ignore fs, gs
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
	add rsp, 8			; ignore rsp
	pop rbx
	pop rdx
	pop rcx
	pop rax
	add rsp, 4 * 8		; ignore int_no, error, rip, cs
	popfq
	add rsp, 2 * 8		; ignore userrsp, ss

    jmp [rsp - 5 * 8]	; jump to rip from saved state

SECTION .data

align 4096
global boot_stack
boot_stack:
    TIMES (MAX_CORES*KERNEL_STACK_SIZE) DB 0xcd
global boot_ist
boot_ist:
    TIMES KERNEL_STACK_SIZE DB 0xcd

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
