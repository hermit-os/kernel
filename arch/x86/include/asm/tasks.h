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
 * @file arch/x86/include/asm/tasks.h
 * @brief Task related structure definitions
 *
 * This file contains the task_t structure definition
 * and task state define constants
 */

#ifndef __ASM_TASKS_H__
#define __ASM_TASKS_H__

#include <hermit/stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Switch to current task
 *
 * @param stack Pointer to the old stack pointer
 */
void switch_context(size_t** stack);

/** @brief Setup a default frame for a new task
 *
 * @param task Pointer to the task structure
 * @param ep The entry point for code execution
 * @param arg Arguments list pointer for the task's stack
 * @param core_id Id of the core, which is firstly used by the task
 * @return
 * - 0 on success
 * - -EINVAL (-22) on failure
 */
int create_default_frame(task_t* task, entry_point_t ep, void* arg, uint32_t core_id);

/** @brief Jump to user code
 *
 * This function runs the user code after stopping it just as if
 * it was a return from a procedure.
 *
 * @return 0 in any case
 */
static inline int jump_to_user_code(size_t ep, size_t stack)
{
	// Create a pseudo interrupt on the stack and return to user function
	asm volatile ("push %0; push %1; push $0x41202; push %2; push %3; iretq" :: "r"(0x33ULL), "r"(stack), "r"(0x2bULL), "r"(ep) : "memory");

	return 0;
}

/** @brief Architecture dependent initialize routine
 */
static inline void arch_init_task(task_t* task)
{
	set_tss((size_t) task->stack + KERNEL_STACK_SIZE - 0x10, (size_t) task->ist_addr + KERNEL_STACK_SIZE - 0x10);
}

#ifdef __cplusplus
}
#endif

#endif
