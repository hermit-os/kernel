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
 * @file arch/x86/include/asm/tasks_types.h
 * @brief Task related structure definitions
 *
 * This file contains the task_t structure definition
 * and task state define constants
 */

#ifndef __ASM_TASKS_TYPES_H__
#define __ASM_TASKS_TYPES_H__

#include <hermit/stddef.h>
#include <asm/processor.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
	uint32_t	cwd;
	uint32_t	swd;
	uint32_t	twd;
	uint32_t	fip;
	uint32_t	fcs;
	uint32_t	foo;
	uint32_t	fos;
	uint32_t	st_space[20];
	uint32_t	status;
} i387_fsave_t;

#define FPU_STATE_INIT { {0, 0, 0, 0, 0, 0, 0, { [0 ... 19] = 0 }, 0} }

typedef struct {
	uint16_t	cwd;
	uint16_t	swd;
	uint16_t	twd;
	uint16_t	fop;
	union {
		struct {
			uint64_t	rip;
			uint64_t	rdp;
		};
		struct {
			uint32_t	fip;
			uint32_t	fcs;
			uint32_t	foo;
			uint32_t	fos;
		};
	};
	uint32_t	mxcsr;
	uint32_t	mxcsr_mask;
	uint32_t	st_space[32];
	uint32_t	xmm_space[64];
	uint32_t	padding[12];
	union {
		uint32_t	padding1[12];
		uint32_t	sw_reserved[12];
	};
} i387_fxsave_t __attribute__ ((aligned (16)));

typedef struct {
	uint64_t xstate_bv;
	uint64_t xcomp_bv;
	uint64_t reserved[6];
} xsave_header_t;

typedef struct {
	uint32_t ymmh_space[64];
} ymmh_t;

typedef struct {
	uint64_t lwpcb_addr;
	uint32_t flags;
	uint32_t buf_head_offset;
	uint64_t buf_base;
	uint32_t buf_size;
	uint32_t filters;
	uint64_t saved_event_record[4];
	uint32_t event_counter[16];
} lwp_t;

typedef struct {
	uint64_t bndregs[8];
} bndregs_t;

typedef struct {
	uint64_t cfg_reg_u;
	uint64_t status_reg;
} bndcsr_t;

typedef struct {
	i387_fxsave_t fxsave;
	xsave_header_t hdr;
	ymmh_t ymmh;
	lwp_t lwp;
	bndregs_t bndregs;
	bndcsr_t bndcsr;
} xsave_t __attribute__ ((aligned (64)));

union fpu_state {
	i387_fsave_t	fsave;
	i387_fxsave_t	fxsave;
	xsave_t xsave;
};

typedef struct {
	uint16_t control_word;
	uint16_t unused1;
	uint16_t status_word;
	uint16_t unused2;
	uint16_t tags;
	uint16_t unused3;
	uint32_t eip;
	uint16_t cs_selector;
	uint32_t opcode:11;
	uint32_t unused4:5;
	uint32_t data_offset;
	uint16_t data_selector;
	uint16_t unused5;
} fenv_t;

typedef struct ucontext {
	mregs_t		uc_mregs;
	fenv_t		uc_fenv;
	struct ucontext	*uc_link;
	stack_t		uc_stack;
} ucontext_t;

typedef void (*handle_fpu_state)(union fpu_state* state);

extern handle_fpu_state save_fpu_state;
extern handle_fpu_state restore_fpu_state;
extern handle_fpu_state fpu_init;

#ifdef __cplusplus
}
#endif

#endif
