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
    mov eax, OFFSET {level_4_table}
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
