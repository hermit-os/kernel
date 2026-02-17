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
    add rdi, OFFSET .LLEVEL_1_TABLE_1
    or rax, 0x3               # set present and writable bits
    mov [rdi], rax
    add rcx, 0x1000
    jmp L0
 L1:
    pop rcx
    pop rbx
    pop rdi

    # Set CR3
    mov rax, OFFSET .LLEVEL_4_TABLE
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
    jmp {loader_main}
invalid:
    jmp invalid

.section .data
.global boot_params
.align 8
boot_params:
    .8byte 0

// Page Tables.
//
// This defines the page tables that we switch to by setting `CR3` to `.LLEVEL_4_TABLE`.

    // Page Table Flags.
    //
    // For details, see <https://github.com/rust-osdev/x86_64/blob/v0.15.4/src/structures/paging/page_table.rs#L136-L199>.
    .equiv PAGE_TABLE_FLAGS_PRESENT, 1
    .equiv PAGE_TABLE_FLAGS_WRITABLE, 1 << 1

    .equiv PAGE_TABLE_ENTRY_COUNT, 512

    .equiv SIZE_4_KIB, 0x1000
    .equiv SIZE_2_MIB, SIZE_4_KIB * PAGE_TABLE_ENTRY_COUNT
    .equiv SIZE_1_GIB, SIZE_2_MIB * PAGE_TABLE_ENTRY_COUNT

    .equiv PAGE_TABLE_FLAGS, PAGE_TABLE_FLAGS_PRESENT | PAGE_TABLE_FLAGS_WRITABLE

    .type .LLEVEL_4_TABLE,@object
    .section .data..LLEVEL_4_TABLE,"awR",@progbits
    .align SIZE_4_KIB
.LLEVEL_4_TABLE:
    .quad .LLEVEL_3_TABLE + PAGE_TABLE_FLAGS
    .fill PAGE_TABLE_ENTRY_COUNT - 2, 8, 0
    .quad .LLEVEL_4_TABLE + PAGE_TABLE_FLAGS
    .size .LLEVEL_4_TABLE, . - .LLEVEL_4_TABLE

    .type .LLEVEL_3_TABLE,@object
    .section .data..LLEVEL_3_TABLE,"awR",@progbits
    .align SIZE_4_KIB
.LLEVEL_3_TABLE:
    .quad .LLEVEL_2_TABLE + PAGE_TABLE_FLAGS
    .fill PAGE_TABLE_ENTRY_COUNT - 1, 8, 0
    .size .LLEVEL_3_TABLE, . - .LLEVEL_3_TABLE

    .type .LLEVEL_2_TABLE,@object
    .section .data..LLEVEL_2_TABLE,"awR",@progbits
    .align SIZE_4_KIB
.LLEVEL_2_TABLE:
    .quad .LLEVEL_1_TABLE_1 + PAGE_TABLE_FLAGS
    .quad .LLEVEL_1_TABLE_2 + PAGE_TABLE_FLAGS
    .fill PAGE_TABLE_ENTRY_COUNT - 2, 8, 0
    .size .LLEVEL_2_TABLE, . - .LLEVEL_2_TABLE

    .type .LLEVEL_1_TABLE_1,@object
    .section .data..LLEVEL_1_TABLE_1,"awR",@progbits
    .align SIZE_4_KIB
.LLEVEL_1_TABLE_1:
    .fill PAGE_TABLE_ENTRY_COUNT, 8, 0
    .size .LLEVEL_1_TABLE_1, . - .LLEVEL_1_TABLE_1

    .type .LLEVEL_1_TABLE_2,@object
    .section .data..LLEVEL_1_TABLE_2,"awR",@progbits
    .align SIZE_4_KIB
.LLEVEL_1_TABLE_2:
    .fill PAGE_TABLE_ENTRY_COUNT, 8, 0
    .size .LLEVEL_1_TABLE_2, . - .LLEVEL_1_TABLE_2
