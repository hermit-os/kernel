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
#include <asm/tss.h>
#include <asm/page.h>

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void percore_start;
extern const void percore_end0;

extern uint64_t base;

static inline void enter_user_task(size_t ep, size_t stack)
{
	// don't interrupt the jump to user-level code
	irq_disable();

	asm volatile ("swapgs" ::: "memory");

	// the jump also enable interrupts
	jump_to_user_code(ep, stack);
}

static int thread_entry(void* arg, size_t ep)
{
#if 0
	size_t addr, stack = 0;
	size_t flags;
	int64_t npages;
	size_t offset = DEFAULT_STACK_SIZE-16;

	//create user-level stack
	npages = DEFAULT_STACK_SIZE >> PAGE_BITS;
	if (DEFAULT_STACK_SIZE & (PAGE_SIZE-1))
		npages++;

	addr = get_pages(npages);
	if (BUILTIN_EXPECT(!addr, 0)) {
		kprintf("load_task: not enough memory!\n");
		return -ENOMEM;
	}

	stack = (1ULL << 34ULL) - curr_task->id*DEFAULT_STACK_SIZE-PAGE_SIZE;	// virtual address of the stack
	flags = PG_USER|PG_RW;
	if (has_nx())
		flags |= PG_XD;

	if (page_map(stack, addr, npages, flags)) {
		put_pages(addr, npages);
		kprintf("Could not map stack at 0x%x\n", stack);
		return -ENOMEM;
	}
	memset((void*) stack, 0x00, npages*PAGE_SIZE);
	//kprintf("stack located at 0x%zx (0x%zx)\n", stack, addr);

	// create vma regions for the user-level stack
	flags = VMA_CACHEABLE|VMA_USER|VMA_READ|VMA_WRITE;
	vma_add(stack, stack+npages*PAGE_SIZE-1, flags);
#endif

	if (init_tls())
		return -ENOMEM;

	//vma_dump();

	// set first argument
	//asm volatile ("mov %0, %%rdi" :: "r"(arg));
	//enter_user_task(ep, stack+offset);

	entry_point_t call_ep = (entry_point_t) ep;
	call_ep(arg);

	return 0;
}

size_t* get_current_stack(void)
{
	task_t* curr_task = per_core(current_task);
	size_t stptr = (size_t) curr_task->stack + KERNEL_STACK_SIZE - 0x10;

	set_per_core(kernel_stack, stptr);
	tss_set_rsp0(stptr);

#if 0
	// do we change the address space?
	if (read_cr3() != curr_task->page_map)
		write_cr3(curr_task->page_map); // use new page table
#endif

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

	kprintf("Task %d use use the memory region [%p - %p] as kernel stack\n", task->id, task->stack, (char*) task->stack + KERNEL_STACK_SIZE - 1);

	memset(task->stack, 0xCD, KERNEL_STACK_SIZE);

	/* The difference between setting up a task for SW-task-switching
	 * and not for HW-task-switching is setting up a stack and not a TSS.
	 * This is the stack which will be activated and popped off for iret later.
	 */
	stack = (size_t*) (((size_t) task->stack + KERNEL_STACK_SIZE - 0x10) & ~0xF);	// => stack is 16byte aligned

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
	//if ((size_t) ep < KERNEL_SPACE) {
	//	stptr->rip = (size_t)ep;
	//} else {
		stptr->rip = (size_t)thread_entry;
		stptr->rsi = (size_t)ep; // use second argument to transfer the entry point
	//}
	stptr->cs = 0x08;
	stptr->ss = 0x10;
	stptr->gs = core_id * ((size_t) &percore_end0 - (size_t) &percore_start);
	stptr->rflags = 0x1202;
	stptr->userrsp = stptr->rsp;

	/* Set the task's stack pointer entry to the stack we have crafted right now. */
	task->last_stack_pointer = (size_t*)stack;

	return 0;
}
