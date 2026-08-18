    /// Moves the address of the provided symbol into the provided register.
    ///
    /// On 32-bit x86, we cannot access eip to do native eip-relative addressing.
    /// Instead, we have to retrieve the eip address via the stack, correct it,
    /// and look up the symbol in the GOT.
.macro movgot32 reg, sym
    call 2f
2:
    pop eax
3:
    // Not supported in Intel syntax on LLVM yet:
    // https://github.com/llvm/llvm-project/issues/161550
.att_syntax prefix
    addl $_GLOBAL_OFFSET_TABLE_ + (3b - 2b), %eax
.intel_syntax noprefix
    mov \reg, dword ptr [eax + \sym@GOT]
.endm
