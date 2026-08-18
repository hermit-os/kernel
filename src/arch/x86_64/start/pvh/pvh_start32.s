    //! 32-bit PVH entry point.
    //!
    //! This 32-bit assembly code configures and switches to the 64-bit mode
    //! and jumps to Rust. ebx contains the start info physical address.

    // Set up the stack
    // For position-independence, push eip below start info
    mov esp, ebx
    movgot32 esp, {stack}
    add esp, {stack_size}

    // Load level 4 page table into cr3
    movgot32 eax, {level_4_table}
    mov cr3, eax

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

    // Load the GDT
    movgot32 eax, {gdt_ptr}
    lgdt [eax]

    // Load the segment registers
    mov ax, {kernel_data_selector}
    mov ds, eax
    mov es, eax
    mov fs, eax
    mov gs, eax
    mov ss, eax

    // Jump to rust_start with start info paddr as first argument
    mov edi, ebx

    // We need to do a far jump, but LLVM does not support absolute far jumps
    // in Intel syntax yet: https://github.com/llvm/llvm-project/issues/46048
    // We could switch to AT&T syntax for the unsupported line of code but
    // using far returns is more flexible anyway.
    push {kernel_code_selector}
    movgot32 eax, {rust_start}
    push eax
    retf
