; This is the entry point for the application processors.
; It is loaded at 0x8000 by HermitCore and filled with parameters.
; It does the switch from Real Mode -> Protected Mode -> Long Mode,
; sets up CR3 for this CPU, and then calls into _start.
;
; In contrast to this self-contained entry point, _start is linked
; to the rest of HermitCore and thus has access to all exported symbols
; (like the actual Rust entry point).


CR0_PG       equ (1 << 31)
CR4_PAE      equ (1 << 5)
MSR_EFER     equ 0xC0000080
EFER_LME     equ (1 << 8)
EFER_NXE     equ (1 << 11)

[BITS 16]
SECTION .text
GLOBAL _start
ORG 0x8000
_start:
	jmp _rmstart

; PARAMETERS
align 8
	entry_point dq 0xDEADC0DE
	cpu_id dd 0xC0DECAFE
	boot_info dq 0xBEEFBEEF
	pml4 dd 0xDEADBEEF
	pad dd 0;

_rmstart:
	cli
	lgdt [gdtr]

	; switch to protected mode by setting PE bit
	mov eax, cr0
	or al, 0x1
	mov cr0, eax

	; far jump to the 32bit code
	jmp dword codesel : _pmstart

[BITS 32]
ALIGN 4
_pmstart:
	xor eax, eax
	mov ax, datasel
	mov ds, ax
	mov es, ax
	mov fs, ax
	mov gs, ax
	mov ss, ax

	jmp short stublet
	jmp $

; GDT for the protected mode
ALIGN 4
gdtr:                           ; descritor table
        dw gdt_end-gdt-1        ; limit
        dd gdt                  ; base address
gdt:
        dd 0,0                  ; null descriptor
codesel equ $-gdt
        dw 0xFFFF               ; segment size 0..15
        dw 0x0000               ; segment address 0..15
        db 0x00                 ; segment address 16..23
        db 0x9A                 ; access permissions und type
        db 0xCF                 ; additional information and segment size 16...19
        db 0x00                 ; segment address 24..31
datasel equ $-gdt
        dw 0xFFFF               ; segment size 0..15
        dw 0x0000               ; segment address 0..15
        db 0x00                 ; segment address 16..23
        db 0x92                 ; access permissions and type
        db 0xCF                 ; additional informationen and degment size 16...19
        db 0x00                 ; segment address 24..31
gdt_end:

ALIGN 4
GDTR64:
    dw GDT64_end - GDT64 - 1     ; Limit.
    dq GDT64                     ; Base.

; we need a new GDT to switch in the 64bit modus
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
GDT64_end:

ALIGN 4
stublet:
    ; Enable PAE mode.
    mov eax, cr4
    or eax, CR4_PAE
    mov cr4, eax

    ; Set the address to PML4 in CR3.
    mov eax, dword [pml4]
    mov cr3, eax

    ; Enable x86-64 Compatibility Mode by setting EFER_LME.
    ; Also enable early access to NO_EXECUTE-protected memory through EFER_NXE.
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_LME | EFER_NXE
    wrmsr

    ; Enable Paging.
    mov eax, cr0
    or eax, CR0_PG
    mov cr0, eax

    ; Load the 64-bit global descriptor table.
    lgdt [GDTR64]
    mov ax, GDT64.Data
    mov ss, ax
    mov ds, ax
    mov es, ax

    ; Set the code segment and enter 64-bit long mode.
    jmp GDT64.Code:start64

[BITS 64]
ALIGN 8
start64:
    ; forward address to boot info
    mov rdi, qword [boot_info]
    mov esi, dword [cpu_id]
    ; Jump to _start
    jmp qword [entry_point]
