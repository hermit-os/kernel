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
 * @file arch/x86/include/asm/tss.h
 * @brief Task state segment structure definition
 */

#ifndef __ARCH_TSS_H__
#define __ARCH_TSS_H__

#include <hermit/stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/** @brief The tast state segment structure
 */
typedef struct {
	uint16_t	res0, res1;		// reserved entries
	uint64_t	rsp0;
	uint64_t	rsp1;
	uint64_t	rsp2;
	uint32_t	res2, res3;		// reserved entries
	uint64_t	ist1;
	uint64_t	ist2;
	uint64_t	ist3;
	uint64_t	ist4;
	uint64_t	ist5;
	uint64_t	ist6;
	uint64_t	ist7;
	uint32_t	res4, res5;		// reserved entries
	uint16_t	res6, bitmap;
} __attribute__ ((packed)) tss_t;

/** @brief Set rsp0 & ist1 in TSS of the current core
 */
void set_tss(size_t rsp0, size_t ist1);

#ifdef __cplusplus
}
#endif

#endif
