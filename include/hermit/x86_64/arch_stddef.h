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

// A popular type for addresses
typedef unsigned long long size_t;
/// Pointer differences
typedef long long ptrdiff_t;
#ifdef __KERNEL__
typedef long long ssize_t;
typedef long long off_t;
#endif

/// Unsigned 64 bit integer
typedef unsigned long uint64_t;
/// Signed 64 bit integer
typedef long int64_t;
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

#ifndef _WINT_T
#define _WINT_T
typedef wchar_t wint_t;
#endif

/// This defines registers, which are saved for a "user-level" context swicth
typedef struct mregs {
	/// R15 register
	uint64_t r15;
	/// R14 register
	uint64_t r14;
	/// R13 register
	uint64_t r13;
	/// R12 register
	uint64_t r12;
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
	/// RBX register
	uint64_t rbx;
	/// RDX register
	uint64_t rdx;
	/// RCX register
	uint64_t rcx;
	/// RSP register
	uint64_t rsp;
	/// RIP
	uint64_t rip;
	/// MXCSR
	uint32_t mxcsr;
} mregs_t;

typedef struct {
	void	*ss_sp;		/* Stack base or pointer.  */
	int	ss_flags;	/* Flags.  */
	size_t	ss_size;	/* Stack size.  */
} stack_t;

#ifdef __cplusplus
}
#endif

#endif
