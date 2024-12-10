# This is the kernel's entry point, if Hermit is running with
# FireCracker. FireCracker assumes a 64 bit Linux kernel.

.code64

.set BOOT_STACK_SIZE, 4096

.extern loader_start		# defined in linker script
.extern loader_end

# Move entry point at the beginning of the elf file
.section .mboot, "a"
.align 8
.global _start
_start:
    cli # avoid any interrupt

    # Initialize stack pointer
    movabs rsp, OFFSET boot_stack
    add rsp, BOOT_STACK_SIZE - 16

    mov [boot_params], rsi

    # initialize page tables
    # map kernel 1:1
    push rdi
    push rbx
    push rcx
    movabs rcx, OFFSET loader_start
    movabs rbx, OFFSET loader_end
    add rbx, 0x1000
 L0: cmp rcx, rbx
    jae L1
    mov rax, rcx
    and eax, 0xFFFFF000       # page align lower half
    mov rdi, rax
    shr rdi, 9                # (edi >> 12) * 8 (index for boot_pgt)
    add rdi, OFFSET boot_pgt1
    or rax, 0x3               # set present and writable bits
    mov [rdi], rax
    add rcx, 0x1000
    jmp L0
 L1:
    pop rcx
    pop rbx
    pop rdi

    # Set CR3
    mov rax, OFFSET boot_pml4
    mov cr3, rax

    lgdt [GDT64.Pointer] # Load the 64-bit global descriptor table.
    jmp start64 # Set the code segment and enter 64-bit long mode.

.section .text
.align 8
start64:
    # initialize segment registers
    mov ax, OFFSET GDT64.Data
    mov ds, ax
    mov es, ax
    mov ss, ax
    xor ax, ax
    mov fs, ax
    mov gs, ax
    cld
    # set default stack pointer
    movabs rsp, OFFSET boot_stack
    add rsp, BOOT_STACK_SIZE-16

    # jump to the boot processors's C code
    .extern loader_main
    jmp loader_main
invalid:
    jmp invalid

.section .data
.align 4
# we need already a valid GDT to switch in the 64bit modus
GDT64:                      # Global Descriptor Table (64-bit).
.set GDT64.Null, . - GDT64  # The null descriptor.
    .2byte 0                # Limit (low).
    .2byte 0                # Base (low).
    .byte 0                 # Base (middle)
    .byte 0                 # Access.
    .byte 0                 # Granularity.
    .byte 0                 # Base (high).
.set GDT64.Code, . - GDT64  # The code descriptor.
    .2byte 0                # Limit (low).
    .2byte 0                # Base (low).
    .byte 0                 # Base (middle)
    .byte 0b10011010        # Access.
    .byte 0b00100000        # Granularity.
    .byte 0                 # Base (high).
.set GDT64.Data, . - GDT64  # The data descriptor.
    .2byte 0                # Limit (low).
    .2byte 0                # Base (low).
    .byte 0                 # Base (middle)
    .byte 0b10010010        # Access.
    .byte 0b00000000        # Granularity.
    .byte 0                 # Base (high).
GDT64.Pointer:              # The GDT-pointer.
    .2byte . - GDT64 - 1    # Limit.
    .8byte GDT64            # Base.

.global boot_params
.align 8
boot_params:
    .8byte 0

.align 4096
.global boot_stack
boot_stack:
    .fill BOOT_STACK_SIZE, 1, 0xcd

# Bootstrap page tables are used during the initialization.
.align 4096
boot_pml4:
    .8byte boot_pdpt + 0x3  # PG_PRESENT | PG_RW
    .fill 510, 8, 0         # PAGE_MAP_ENTRIES - 2
    .8byte boot_pml4 + 0x3  # PG_PRESENT | PG_RW
boot_pdpt:
    .8byte boot_pgd + 0x3   # PG_PRESENT | PG_RW
    .fill 511, 8, 0         # PAGE_MAP_ENTRIES - 1
boot_pgd:
    .8byte boot_pgt1 + 0x3  # PG_PRESENT | PG_RW
    .8byte boot_pgt2 + 0x3  # PG_PRESENT | PG_RW
    .fill 510, 8, 0         # PAGE_MAP_ENTRIES - 1
boot_pgt1:
    .fill 512, 8, 0
boot_pgt2:
    .fill 512, 8, 0
