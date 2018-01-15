
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
