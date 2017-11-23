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

MSR_FS_BASE equ 0xc0000100

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


; Required for Go applications
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

; Required for Go applications
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

; Required for Go applications
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


global switch
align 8
switch:
	; rdi => the address to store the old rsp
	; rsi => stack pointer of the new task

	; save context
	pushfq							; push control register
	push rax
	push rcx
	push rdx
	push rbx
	push rsp						; determine rsp before storing the context
	add QWORD [rsp], 6*8
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

	; push the fs register used for Thread-Local Storage
global fs_patch0
fs_patch0:
	jmp short rdfs_old_way
	rdfsbase rax
	push rax
	jmp short fs_saved
rdfs_old_way:
	mov ecx, MSR_FS_BASE
	rdmsr
	sub rsp, 8
	mov DWORD [rsp+4], edx
	mov DWORD [rsp], eax
fs_saved:

	mov QWORD [rdi], rsp			; store old rsp
	mov rsp, rsi

	; Set task switched flag
	mov rax, cr0
	or rax, 8
	mov cr0, rax

	; set stack pointer in TSS
	extern set_current_kernel_stack
	call set_current_kernel_stack

	; restore the fs register
global fs_patch1
fs_patch1:
	jmp short wrfs_old_way
	pop rax
	wrfsbase rax
	jmp short fs_restored
wrfs_old_way:
	mov ecx, MSR_FS_BASE
	mov edx, DWORD [rsp+4]
	mov eax, DWORD [rsp]
	wrmsr
	add esp, 8
fs_restored:

	; restore remaining context
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
	popfq

	ret

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

; add some hints to the ELF file
SECTION .note.GNU-stack noalloc noexec nowrite progbits
