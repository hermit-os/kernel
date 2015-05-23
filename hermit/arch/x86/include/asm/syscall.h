/*
 * Copyright (c) 2011, Stefan Lankes, RWTH Aachen University
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
 * @file arch/x86/include/asm/syscall.h
 * @brief Systemcall related code
 *
 * This file defines the syscall function and convenience 
 * based macro definitions for calling it.
 */

#ifndef __ARCH_SYSCALL_H__
#define __ARCH_SYSCALL_H__

#include <hermit/stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define _STR(token)		#token
#define _SYSCALLSTR(x)	"int $" _STR(x) " "

/** @brief the syscall function which issues an interrupt to the kernel
 *
 * It's supposed to be used by the macros defined in this file as the could would read
 * cleaner then.
 *
 * @param nr System call number
 * @param arg0 Argument 0
 * @param arg1 Argument 1
 * @param arg2 Argument 2
 * @param arg3 Argument 3
 * @param arg4 Argument 4
 * @return The return value of the system call
 */
inline static long
syscall(int nr, unsigned long arg0, unsigned long arg1, unsigned long arg2,
	unsigned long arg3, unsigned long arg4)
{
	long res;

	asm volatile ("mov %5, %%r8; mov %6, %%r9; syscall"
			: "=a" (res)
			: "D" (nr), "S" (arg0), "d" (arg1), "c" (arg2), "m" (arg3), "m" (arg4)
			: "memory", "cc", "%r8", "%r9");

	return res;
}

/// System call macro with one single argument; the syscall number
#define SYSCALL0(NR) \
	syscall(NR, 0, 0, 0, 0, 0)
#define SYSCALL1(NR, ARG0) \
	syscall(NR, (unsigned long)ARG0, 0, 0, 0, 0)
#define SYSCALL2(NR, ARG0, ARG1) \
	syscall(NR, (unsigned long)ARG0, (unsigned long)ARG1, 0, 0, 0)
#define SYSCALL3(NR, ARG0, ARG1, ARG2) \
	syscall(NR, (unsigned long)ARG0, (unsigned long)ARG1, (unsigned long)ARG2, 0, 0)
#define SYSCALL4(NR, ARG0, ARG1, ARG2, ARG3) \
	syscall(NR, (unsigned long)ARG0, (unsigned long)ARG1, (unsigned long)ARG2, (unsigned long) ARG3, 0)
#define SYSCALL5(NR, ARG0, ARG1, ARG2, ARG3, ARG4) \
	syscall(NR, (unsigned long)ARG0, (unsigned long)ARG1, (unsigned long)ARG2, (unsigned long) ARG3, (unsigned long) ARG4)

#ifdef __cplusplus
}
#endif

#endif
