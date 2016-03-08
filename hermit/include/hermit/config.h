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

#ifndef __CONFIG_H__
#define __CONFIG_H__

#ifdef __cplusplus
extern "C" {
#endif

#define MAX_CORES		64
#define MAX_TASKS		(MAX_CORES*2+2)
#define MAX_ISLE		8
#define MAX_FNAME		128
#define TIMER_FREQ		100 /* in HZ */
#define CLOCK_TICK_RATE		1193182 /* 8254 chip's internal oscillator frequency */
#define VIDEO_MEM_ADDR		0xB8000 /* the video memory address */
#define CACHE_LINE		64
#define KERNEL_STACK_SIZE	(8*1024)
#define DEFAULT_STACK_SIZE	(64*1024*1024)
#define KMSG_SIZE		(4*1024)
#define INT_SYSCALL		0x80
#define MAILBOX_SIZE		128
//#define WITH_PCI_IDS
//#define SAVE_FPU

#define BYTE_ORDER		LITTLE_ENDIAN

#define LIBOS
#define DYNAMIC_TICKS

#define BUILTIN_EXPECT(exp, b) 	__builtin_expect((exp), (b))
//#define BUILTIN_EXPECT(exp, b)	(exp)
#define NORETURN 		__attribute__((noreturn))

#define HAVE_ARCH_MEMSET
#define HAVE_ARCH_MEMCPY
#define HAVE_ARCH_STRLEN
#define HAVE_ARCH_STRCPY
#define HAVE_ARCH_STRNCPY

#ifdef __cplusplus
}
#endif

#endif
