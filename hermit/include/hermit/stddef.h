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

#define NULL 		((void*) 0)

/// represents a task identifier
typedef unsigned int tid_t;

#if MAX_CORES == 1
#define per_core(name) name
#define DECLARE_PER_CORE(type, name) extern type name;
#define DEFINE_PER_CORE(type, name, def_value) type name = def_value;
#define DEFINE_PER_CORE_STATIC(type, name, def_value)   static type name = def_value;
#define CORE_ID 0
#else
#define per_core(name) (*__get_percore_##name())
#define DECLARE_PER_CORE(type, name) \
	typedef struct { type var  __attribute__ ((aligned (CACHE_LINE))); } aligned_##name;\
	extern aligned_##name name[MAX_CORES];\
	inline static type* __get_percore_##name(void) {\
		type* ret; \
		uint8_t flags = irq_nested_disable(); \
		ret = &(name[smp_id()].var); \
		irq_nested_enable(flags);\
		return ret; \
	}
#define DEFINE_PER_CORE(type, name, def_value) \
	aligned_##name name[MAX_CORES] = {[0 ... MAX_CORES-1] = {def_value}};
#define DEFINE_PER_CORE_STATIC(type, name, def_value) \
	typedef struct { type var  __attribute__ ((aligned (CACHE_LINE))); } aligned_##name;\
	static aligned_##name name[MAX_CORES] = {[0 ... MAX_CORES-1] = {def_value}}; \
	inline static type* __get_percore_##name(void) {\
		type* ret; \
		uint8_t flags = irq_nested_disable(); \
		ret = &(name[smp_id()].var); \
		irq_nested_enable(flags);\
		return ret; \
	}
#define CORE_ID smp_id()
#endif

/* needed to find the task, which is currently running on this core */
struct task;
DECLARE_PER_CORE(struct task*, current_task);

#ifdef __cplusplus
}
#endif

#endif
