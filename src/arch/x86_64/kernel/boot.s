# This is the entry point for the application processors.
# It is loaded at 0x8000 by Hermit and filled with parameters.
# It does the switch from Real Mode -> Protected Mode -> Long Mode,
# sets up CR3 for this CPU, and then calls into _start.
#
# In contrast to this self-contained entry point, _start is linked
# to the rest of Hermit and thus has access to all exported symbols
# (like the actual Rust entry point).

.intel_syntax noprefix

.set CR0_PG,    1 << 31
.set CR4_PAE,   1 << 5
.set MSR_EFER,  0xC0000080
.set EFER_LME,  1 << 8
.set EFER_NXE,  1 << 11

.code16
.section .text
.global _start
_start:
    jmp _rmstart

# PARAMETERS
.align 8
    entry_point:    .8byte 0xDEADC0DE
    cpu_id:         .4byte 0xC0DECAFE
    boot_info:      .8byte 0xBEEFBEEF
    pml4:           .4byte 0xDEADBEEF
    pad:            .4byte 0

_rmstart:
    cli
    lgdt [gdtr]

    # switch to protected mode by setting PE bit
    mov eax, cr0
    or al, 0x1
    mov cr0, eax

    # https://github.com/llvm/llvm-project/issues/46048
    .att_syntax prefix
    # far jump to the 32bit code
    ljmpl $codesel, $_pmstart
    .intel_syntax noprefix

.code32
.align 4
_pmstart:
    xor eax, eax
    mov ax, OFFSET datasel
    mov ds, eax
    mov es, eax
    mov fs, eax
    mov gs, eax
    mov ss, eax

    jmp short stublet
2:
    jmp 2b

# GDT for the protected mode
.align 4
gdtr:                           # descritor table
    .2byte  gdt_end - gdt - 1   # limit
    .4byte  gdt                 # base address
gdt:
    .8byte  0                   # null descriptor
.set codesel, . - gdt
    .2byte  0xFFFF              # segment size 0..15
    .2byte  0                   # segment address 0..15
    .byte   0                   # segment address 16..23
    .byte   0x9A                # access permissions und type
    .byte   0xCF                # additional information and segment size 16...19
    .byte   0                   # segment address 24..31
.set datasel, . - gdt
    .2byte  0xFFFF              # segment size 0..15
    .2byte  0                   # segment address 0..15
    .byte   0                   # segment address 16..23
    .byte   0x92                # access permissions and type
    .byte   0xCF                # additional informationen and degment size 16...19
    .byte   0                   # segment address 24..31
gdt_end:

.align 4
GDTR64:
    .2byte GDT64_end - GDT64 - 1    # Limit.
    .8byte GDT64                    # Base.

# we need a new GDT to switch in the 64bit modus
GDT64:                              # Global Descriptor Table (64-bit).
.set GDT64.Null, . - GDT64          # The null descriptor.
    .2byte  0                       # Limit (low).
    .2byte  0                       # Base (low).
    .byte   0                       # Base (middle)
    .byte   0                       # Access.
    .byte   0                       # Granularity.
    .byte   0                       # Base (high).
.set GDT64.Code, . - GDT64          # The code descriptor.
    .2byte  0                       # Limit (low).
    .2byte  0                       # Base (low).
    .byte   0                       # Base (middle)
    .byte   0b10011010              # Access.
    .byte   0b00100000              # Granularity.
    .byte   0                       # Base (high).
.set GDT64.Data, . - GDT64          # The data descriptor.
    .2byte  0                       # Limit (low).
    .2byte  0                       # Base (low).
    .byte   0                       # Base (middle)
    .byte   0b10010010              # Access.
    .byte   0b00000000              # Granularity.
    .byte   0                       # Base (high).
GDT64_end:

.align 4
stublet:
    # Enable PAE mode.
    mov eax, cr4
    or eax, CR4_PAE
    mov cr4, eax

    # Set the address to PML4 in CR3.
    mov eax, [pml4]
    mov cr3, eax

    # Enable x86-64 Compatibility Mode by setting EFER_LME.
    # Also enable early access to NO_EXECUTE-protected memory through EFER_NXE.
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_LME | EFER_NXE
    wrmsr

    # Enable Paging.
    mov eax, cr0
    or eax, CR0_PG
    mov cr0, eax

    # Load the 64-bit global descriptor table.
    lgdt [GDTR64]
    mov ax, OFFSET GDT64.Data
    mov ss, eax
    mov ds, eax
    mov es, eax

    # https://github.com/llvm/llvm-project/issues/46048
    .att_syntax prefix
    # Set the code segment and enter 64-bit long mode.
    ljmpl $GDT64.Code, $start64
    .intel_syntax noprefix

.code64
.align 8
start64:
    # forward address to boot info
    mov rdi, [boot_info]
    mov esi, [cpu_id]
    # Jump to _start
    jmp [entry_point]
