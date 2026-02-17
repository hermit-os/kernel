# This is the kernel's entry point. We could either call main here,
# or we can use this to setup the stack or other nice stuff, like
# perhaps setting up the GDT and segments. Please note that interrupts
# are disabled at this point: More on interrupts later!

.code32

.extern loader_start		# defined in linker script
.extern loader_end

# We use a special name to map this section at the begin of our kernel
# =>  Multiboot expects its magic number at the beginning of the kernel.
.section .mboot, "a"

# This part MUST be 4 byte aligned, so we solve that issue using '.align 4'.
.align 4
mboot:
    # Multiboot macros to make a few lines more readable later
    .set MULTIBOOT_PAGE_ALIGN,    (1 << 0)
    .set MULTIBOOT_MEMORY_INFO,   (1 << 1)
    .set MULTIBOOT_HEADER_MAGIC,  0x1BADB002
    .set MULTIBOOT_HEADER_FLAGS,  MULTIBOOT_PAGE_ALIGN | MULTIBOOT_MEMORY_INFO
    .set MULTIBOOT_CHECKSUM,      -(MULTIBOOT_HEADER_MAGIC + MULTIBOOT_HEADER_FLAGS)

    # This is the GRUB Multiboot header. A boot signature
    .4byte MULTIBOOT_HEADER_MAGIC
    .4byte MULTIBOOT_HEADER_FLAGS
    .4byte MULTIBOOT_CHECKSUM
    .4byte 0, 0, 0, 0, 0 # address fields

.section .text
.align 4
.global _start
_start:
    cli # avoid any interrupt

    # Initialize stack pointer
    mov esp, OFFSET {stack}
    add esp, {stack_top_offset}

    # Interpret multiboot information
    mov [mb_info], ebx

# This will set up the x86 control registers:
# Caching and the floating point unit are enabled
# Bootstrap page tables are loaded and page size
# extensions (huge pages) enabled.
cpu_init:

    # initialize page tables
    # map kernel 1:1
    push edi
    push ebx
    push ecx
    mov ecx, OFFSET loader_start
    mov ebx, OFFSET loader_end
    add ebx, 0x1000
L0: cmp ecx, ebx
    jae L1
    mov eax, ecx
    and eax, 0xFFFFF000       # page align lower half
    mov edi, eax
    shr edi, 9                # (edi >> 12) * 8 (index for boot_pgt)
    add edi, OFFSET .LLEVEL_1_TABLE_1
    or eax, 0x3               # set present and writable bits
    mov [edi], eax
    add ecx, 0x1000
    jmp L0
L1:
    pop ecx
    pop ebx
    pop edi

    # check for long mode

    # do we have the instruction cpuid?
    pushfd
    pop eax
    mov ecx, eax
    xor eax, 1 << 21
    push eax
    popfd
    pushfd
    pop eax
    push ecx
    popfd
    xor eax, ecx
    jz Linvalid

    # cpuid > 0x80000000?
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb Linvalid # It is less, there is no long mode.

    # do we have a long mode?
    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29 # Test if the LM-bit, which is bit 29, is set in the D-register.
    jz Linvalid # They aren't, there is no long mode.

    # Set CR3
    mov eax, OFFSET .LLEVEL_4_TABLE
    mov cr3, eax

    # we need to enable PAE modus
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    # switch to the compatibility mode (which is part of long mode)
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    # Set CR4
    mov eax, cr4
    and eax, 0xfffbf9ff     # disable SSE
    # or eax, (1 << 7)      # enable PGE
    mov cr4, eax

    # Set CR0 (PM-bit is already set)
    mov eax, cr0
    and eax, ~(1 << 2)      # disable FPU emulation
    or eax, (1 << 1)        # enable FPU montitoring
    and eax, ~(1 << 30)     # enable caching
    and eax, ~(1 << 29)     # disable write through caching
    and eax, ~(1 << 16)	    # allow kernel write access to read-only pages
    or eax, (1 << 31)       # enable paging
    mov cr0, eax

    lgdt [{gdt_ptr}] # Load the 64-bit global descriptor table.
    # https://github.com/llvm/llvm-project/issues/46048
    .att_syntax prefix
    # Set the code segment and enter 64-bit long mode.
    ljmp ${kernel_code_selector}, $start64
    .intel_syntax noprefix

# there is no long mode
Linvalid:
    jmp Linvalid

.code64
start64:
    # initialize segment registers
    mov ax, {kernel_data_selector}
    mov ds, eax
    mov es, eax
    mov ss, eax
    xor ax, ax
    mov fs, eax
    mov gs, eax
    cld
    # set default stack pointer
    movabs rsp, OFFSET {stack}
    add rsp, {stack_top_offset}

    # jump to the boot processors's C code
    jmp {loader_main}
    jmp start64+0x28

.section .data
.global mb_info
.align 8
mb_info:
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

    .align SIZE_4_KIB
.LLEVEL_4_TABLE:
    .quad .LLEVEL_3_TABLE + PAGE_TABLE_FLAGS
    .fill PAGE_TABLE_ENTRY_COUNT - 2, 8, 0
    .quad .LLEVEL_4_TABLE + PAGE_TABLE_FLAGS
.LLEVEL_3_TABLE:
    .quad .LLEVEL_2_TABLE + PAGE_TABLE_FLAGS
    .fill PAGE_TABLE_ENTRY_COUNT - 1, 8, 0
.LLEVEL_2_TABLE:
    .quad .LLEVEL_1_TABLE_1 + PAGE_TABLE_FLAGS
    .quad .LLEVEL_1_TABLE_2 + PAGE_TABLE_FLAGS
    .fill PAGE_TABLE_ENTRY_COUNT - 2, 8, 0
.LLEVEL_1_TABLE_1:
    .fill PAGE_TABLE_ENTRY_COUNT, 8, 0
.LLEVEL_1_TABLE_2:
    .fill PAGE_TABLE_ENTRY_COUNT, 8, 0
