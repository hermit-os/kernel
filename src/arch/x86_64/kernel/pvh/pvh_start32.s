    // Load level 4 page table into cr3
    mov eax, offset {level_4_table}
    mov cr3, eax
    cli

    // Enable physical address extensions
.set CR4_FLAGS_PHYSICAL_ADDRESS_EXTENSION, 1 << 5
    mov eax, cr4
    or eax, CR4_FLAGS_PHYSICAL_ADDRESS_EXTENSION
    mov cr4, eax

    // Switch to compatibility mode
.set EFER_MSR, 0xc0000080
.set EFER_FLAGS_LONG_MODE_ENABLE, 1 << 8
    mov ecx, EFER_MSR
    rdmsr
    or eax, EFER_FLAGS_LONG_MODE_ENABLE
    wrmsr

    // Enable paging and protected mode
.set CR0_FLAGS_PROTECTED_MODE_ENABLE, 1
.set CR0_FLAGS_PAGING, 1 << 31
    mov eax, cr0
    or eax, CR0_FLAGS_PROTECTED_MODE_ENABLE | CR0_FLAGS_PAGING
    mov cr0, eax


    # Set CR0 (PM-bit is already set)
    mov eax, cr0
    and eax, ~(1 << 2)      # disable FPU emulation
    or eax, (1 << 1)        # enable FPU montitoring
    and eax, ~(1 << 30)     # enable caching
    and eax, ~(1 << 29)     # disable write through caching
    and eax, ~(1 << 16)	    # allow kernel write access to read-only pages
    or eax, (1 << 31)       # enable paging
    mov cr0, eax

    // Load the GDT
    lgdt [offset {gdt_ptr}]

    // Load the segment registers
    mov ax, {kernel_data_selector}
    mov ds, eax
    mov es, eax
    mov fs, eax
    mov gs, eax
    mov ss, eax

    // ebx contains the start info physical address
    // Jump to rust_start with paddr as first argument
    mov edi, ebx

    // Set up the stack
    mov esp, offset {stack}
    add esp, {stack_size}

    // We need to do a far jump, but LLVM does not support absolute far jumps
    // in Intel syntax yet: https://github.com/llvm/llvm-project/issues/46048
    // We could switch to AT&T syntax for the unsupported line of code but
    // using far returns is more flexible anyway.
    push {kernel_code_selector}
    mov eax, offset {rust_start}
    push eax
    retf
