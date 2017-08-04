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

#ifndef __STDDEF_H__
#define __STDDEF_H__

/**
 * @author Stefan Lankes
 * @file include/hermit/stddef.h
 * @brief Definition of basic data types
 */

#include <hermit/config.h>
#include <asm/stddef.h>
#include <asm/irqflags.h>

#ifdef __cplusplus
extern "C" {
#endif

// size of the whole application
extern const size_t image_size;

#define TIMER_FREQ	100 /* in HZ */
#define CLOCK_TICK_RATE	1193182 /* 8254 chip's internal oscillator frequency */
#define CACHE_LINE	64
#define HEAP_START	(PAGE_2M_CEIL((size_t)&kernel_start + image_size) + 4*PAGE_SIZE)
#define HEAP_SIZE	(1ULL << 32)
#define KMSG_SIZE	0x1000
#define INT_SYSCALL	0x80
#define MAILBOX_SIZE	128
//#define WITH_PCI_IDS

#define BYTE_ORDER             LITTLE_ENDIAN

#define DYNAMIC_TICKS

#define UHYVE_PORT_WRITE	0x499
#define UHYVE_PORT_OPEN		0x500
#define UHYVE_PORT_CLOSE	0x501
#define UHYVE_PORT_READ		0x502
#define UHYVE_PORT_EXIT		0x503
#define UHYVE_PORT_LSEEK	0x504

#define BUILTIN_EXPECT(exp, b)		__builtin_expect((exp), (b))
//#define BUILTIN_EXPECT(exp, b)	(exp)
#define NORETURN			__attribute__((noreturn))

#define NULL 		((void*) 0)

/// represents a task identifier
typedef unsigned int tid_t;

#define DECLARE_PER_CORE(type, name) extern type name __attribute__ ((section (".percore")))
#define DEFINE_PER_CORE(type, name, def_value) type name __attribute__ ((section (".percore"))) = def_value
#define DEFINE_PER_CORE_STATIC(type, name, def_value) static type name __attribute__ ((section (".percore"))) = def_value

/* needed to find the task, which is currently running on this core */
struct task;
DECLARE_PER_CORE(struct task*, current_task);

#if MAX_CORES > 1
/* allows fast access to the core id */
DECLARE_PER_CORE(uint32_t, __core_id);
#define CORE_ID per_core(__core_id)
#else
#define CORE_ID 0
#endif

#ifdef __cplusplus
}
#endif

#endif
