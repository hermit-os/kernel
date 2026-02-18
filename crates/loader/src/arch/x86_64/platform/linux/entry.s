# This is the kernel's entry point, if Hermit is running with
# FireCracker. FireCracker assumes a 64 bit Linux kernel.

.code64

.extern loader_start		# defined in linker script
.extern loader_end

# Move entry point at the beginning of the elf file
.section .mboot, "a"
.align 8
.global _start
_start:
    cli # avoid any interrupt

    # Initialize stack pointer
    movabs rsp, OFFSET {stack}
    add rsp, {stack_top_offset}

    # Move the base address of the struct boot_params into `RDI` as first argument to `rust_start`.
    mov rdi, rsi

    # Set CR3
    mov rax, OFFSET {level_4_table}
    mov cr3, rax

    lgdt [{gdt_ptr}] # Load the 64-bit global descriptor table.
    # CS should already be set to SegmentSelector ( index: 1, rpl: Ring0 )
    # {kernel_code_selector}
    jmp start64

.section .text
.align 8
start64:
    # initialize segment registers
    mov ax, {kernel_data_selector}
    mov ds, ax
    mov es, ax
    mov ss, ax
    xor ax, ax
    mov fs, ax
    mov gs, ax
    cld
    # set default stack pointer
    movabs rsp, OFFSET {stack}
    add rsp, {stack_top_offset}

    # jump to the boot processors's C code
    jmp {rust_start}
invalid:
    jmp invalid
