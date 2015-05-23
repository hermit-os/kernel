/*
 * Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *    * Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    * Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *    * Neither the name of the University nor the names of its contributors
 *      may be used to endorse or promote products derived from this
 *      software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/**
 * @author Stefan Lankes
 * @file arch/x86/include/asm/stddef.h
 * @brief Standard datatypes
 *
 * This file contains typedefs for standard datatypes for numerical and character values.
 */

#ifndef __ARCH_STDDEF_H__
#define __ARCH_STDDEF_H__

#ifdef __cplusplus
extern "C" {
#endif

#if __SIZEOF_POINTER__ == 4

#define KERNEL_SPACE	(1UL << 30) /*  1 GiB */

/// This type is used to represent the size of an object.
typedef unsigned long size_t;
/// Pointer differences
typedef long ptrdiff_t;
/// It is similar to size_t, but must be a signed type.
typedef long ssize_t;
/// The type represents an offset and is similar to size_t, but must be a signed type.
typedef long off_t;
#elif __SIZEOF_POINTER__ == 8

#define KERNEL_SPACE (1ULL << 30)

// A popular type for addresses
typedef unsigned long long size_t;
/// Pointer differences
typedef long long ptrdiff_t;
#ifdef __KERNEL__
typedef long long ssize_t;
typedef long long off_t;
#endif
#else
#error unsupported architecture
#endif

/// Unsigned 64 bit integer
typedef unsigned long long uint64_t;
/// Signed 64 bit integer
typedef long long int64_t;
/// Unsigned 32 bit integer
typedef unsigned int uint32_t;
/// Signed 32 bit integer
typedef int int32_t;
/// Unsigned 16 bit integer
typedef unsigned short uint16_t;
/// Signed 16 bit integer
typedef short int16_t;
/// Unsigned 8 bit integer (/char)
typedef unsigned char uint8_t;
/// Signed 8 bit integer (/char)
typedef char int8_t;
/// 16 bit wide char type
typedef unsigned short wchar_t;

/// This defines what the stack looks like after the task context is saved.
struct state {
	/// R15 register
	uint64_t r15;
	/// R14 register
	uint64_t r14;
	/// R13 register
	uint64_t r13;
	/// R12 register
	uint64_t r12;
	/// R11 register
	uint64_t r11;
	/// R10 register
	uint64_t r10;
	/// R9 register
	uint64_t r9;
	/// R8 register
	uint64_t r8;
	/// RDI register
	uint64_t rdi;
	/// RSI register
	uint64_t rsi;
	/// RBP register
	uint64_t rbp;
	/// (pseudo) RSP register
	uint64_t rsp;
	/// RBX register
	uint64_t rbx;
	/// RDX register
	uint64_t rdx;
	/// RCX register
	uint64_t rcx;
	/// RAX register
	uint64_t rax;

	/// Interrupt number
	uint64_t int_no;

	// pushed by the processor automatically
	uint64_t error;
	uint64_t rip;
	uint64_t cs;
	uint64_t rflags;
	uint64_t userrsp;
	uint64_t ss;
};

#ifdef __cplusplus
}
#endif

#endif
