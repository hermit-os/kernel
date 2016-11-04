/*
 * Copyright (c) 2010-2015, Stefan Lankes, RWTH Aachen University
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

#include <hermit/stdio.h>
#include <hermit/stdlib.h>
#include <hermit/string.h>
#include <hermit/tasks.h>
#include <hermit/errno.h>
#include <hermit/processor.h>
#include <hermit/memory.h>
#include <hermit/vma.h>
#include <hermit/rcce.h>
#include <hermit/logging.h>
#include <asm/tss.h>
#include <asm/page.h>

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void percore_start;
extern const void percore_end0;

extern uint64_t base;

static int thread_entry(void* arg, size_t ep)
{

	if (init_tls())
		return -ENOMEM;

	//vma_dump();

	entry_point_t call_ep = (entry_point_t) ep;
	call_ep(arg);

	return 0;
}

size_t* get_current_stack(void)
{
	task_t* curr_task = per_core(current_task);
	size_t stptr = (size_t) curr_task->stack;

	if (curr_task->status == TASK_IDLE)
		stptr += KERNEL_STACK_SIZE - 0x10;
	else
		stptr = (stptr + DEFAULT_STACK_SIZE - sizeof(size_t)) & ~0x1F;

	set_per_core(kernel_stack, stptr);
	set_tss(stptr, (size_t) curr_task->ist_addr + KERNEL_STACK_SIZE - 0x10);

	return curr_task->last_stack_pointer;
}

int create_default_frame(task_t* task, entry_point_t ep, void* arg, uint32_t core_id)
{
	size_t *stack;
	struct state *stptr;
	size_t state_size;

	if (BUILTIN_EXPECT(!task, 0))
		return -EINVAL;

	if (BUILTIN_EXPECT(!task->stack, 0))
		return -EINVAL;

	LOG_INFO("Task %d uses memory region [%p - %p] as stack\n", task->id, task->stack, (char*) task->stack + KERNEL_STACK_SIZE - 1);
	LOG_INFO("Task %d uses memory region [%p - %p] as IST1\n", task->id, task->ist_addr, (char*) task->ist_addr + KERNEL_STACK_SIZE - 1);

	memset(task->stack, 0xCD, DEFAULT_STACK_SIZE);

	/* The difference between setting up a task for SW-task-switching
	 * and not for HW-task-switching is setting up a stack and not a TSS.
	 * This is the stack which will be activated and popped off for iret later.
	 */
	stack = (size_t*) (((size_t) task->stack + DEFAULT_STACK_SIZE - sizeof(size_t)) & ~0x1F);	// => stack is 32byte aligned

	/* Only marker for debugging purposes, ... */
	*stack-- = 0xDEADBEEF;

	/* and the "caller" we shall return to.
	 * This procedure cleans the task after exit. */
	*stack = (size_t) leave_kernel_task;

	/* Next bunch on the stack is the initial register state.
	 * The stack must look like the stack of a task which was
	 * scheduled away previously. */
	state_size = sizeof(struct state);
	stack = (size_t*) ((size_t) stack - state_size);

	stptr = (struct state *) stack;
	memset(stptr, 0x00, state_size);
	stptr->rsp = (size_t)stack + state_size;
	/* the first-function-to-be-called's arguments, ... */
	stptr->rdi = (size_t) arg;
	stptr->int_no = 0xB16B00B5;
	stptr->error =  0xC03DB4B3;

	/* The instruction pointer shall be set on the first function to be called
	   after IRETing */
	stptr->rip = (size_t)thread_entry;
	stptr->rsi = (size_t)ep; // use second argument to transfer the entry point

	stptr->cs = 0x08;
	stptr->ss = 0x10;
	stptr->gs = core_id * ((size_t) &percore_end0 - (size_t) &percore_start);
	stptr->rflags = 0x1202;
	stptr->userrsp = stptr->rsp;

	/* Set the task's stack pointer entry to the stack we have crafted right now. */
	task->last_stack_pointer = (size_t*)stack;

	return 0;
}

void wait_for_task(void)
{
	if (!has_mwait()) {
		PAUSE;
	} else {
		void* queue = get_readyqueue();

		if (has_clflush())
			clflush(queue);

		monitor(queue, 0, 0);
		mwait(0xF /* = c0 */, 1 /* break on interrupt flag */);
	}
}
