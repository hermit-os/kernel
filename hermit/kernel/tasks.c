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

#include <hermit/stddef.h>
#include <hermit/stdlib.h>
#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/tasks.h>
#include <hermit/tasks_types.h>
#include <hermit/spinlock.h>
#include <hermit/errno.h>
#include <hermit/syscall.h>
#include <hermit/memory.h>

/** @brief Array of task structures (aka PCB)
 *
 * A task's id will be its position in this array.
 */
static task_t task_table[MAX_TASKS] = { \
		[0]                 = {0, TASK_IDLE, 0, NULL, NULL, TASK_DEFAULT_FLAGS, 0, 0, SPINLOCK_IRQSAVE_INIT, SPINLOCK_INIT, NULL, NULL, ATOMIC_INIT(0), NULL, NULL}, \
		[1 ... MAX_TASKS-1] = {0, TASK_INVALID, 0, NULL, NULL, TASK_DEFAULT_FLAGS, 0, 0, SPINLOCK_IRQSAVE_INIT, SPINLOCK_INIT, NULL, NULL,ATOMIC_INIT(0), NULL, NULL}};

static spinlock_irqsave_t table_lock = SPINLOCK_IRQSAVE_INIT;

#if MAX_CORES > 1
static readyqueues_t readyqueues[MAX_CORES] = { \
		[0 ... MAX_CORES-1]   = {NULL, NULL, 0, 0, {[0 ... MAX_PRIO-2] = {NULL, NULL}}, SPINLOCK_IRQSAVE_INIT}};
#else
static readyqueues_t readyqueues[1] = {[0] = {task_table+0, NULL, 0, 0, {[0 ... MAX_PRIO-2] = {NULL, NULL}}, SPINLOCK_IRQSAVE_INIT}};
#endif

DEFINE_PER_CORE(task_t*, current_task, task_table+0);
extern const void boot_stack;

/** @brief helper function for the assembly code to determine the current task
 * @return Pointer to the task_t structure of current task
 */
task_t* get_current_task(void)
{
	return per_core(current_task);
}

uint32_t get_highest_priority(void)
{
	return msb(readyqueues[CORE_ID].prio_bitmap);
}

int multitasking_init(void)
{
	if (BUILTIN_EXPECT(task_table[0].status != TASK_IDLE, 0)) {
		kputs("Task 0 is not an idle task\n");
		return -ENOMEM;
	}

	task_table[0].prio = IDLE_PRIO;
	task_table[0].stack = (char*) &boot_stack;
	task_table[0].page_map = read_cr3();

	readyqueues[CORE_ID].idle = task_table+0;

	// register idle task
	register_task();

	return 0;
}

void finish_task_switch(void)
{
	task_t* old;
	uint8_t prio;
	const uint32_t core_id = CORE_ID;
	task_t* curr_task = per_core(current_task);

	spinlock_irqsave_lock(&readyqueues[core_id].lock);

	if ((old = readyqueues[core_id].old_task) != NULL) {
		if (old->status == TASK_INVALID) {
			old->stack = NULL;
			old->last_stack_pointer = NULL;
			readyqueues[core_id].old_task = NULL;
		} else {
			prio = old->prio;
			if (!readyqueues[core_id].queue[prio-1].first) {
				old->next = old->prev = NULL;
				readyqueues[core_id].queue[prio-1].first = readyqueues[core_id].queue[prio-1].last = old;
			} else {
				old->next = NULL;
				old->prev = readyqueues[core_id].queue[prio-1].last;
				readyqueues[core_id].queue[prio-1].last->next = old;
				readyqueues[core_id].queue[prio-1].last = old;
			}
			readyqueues[core_id].old_task = NULL;
			readyqueues[core_id].prio_bitmap |= (1 << prio);
		}
	}

	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	if (curr_task->heap)
		kfree(curr_task->heap);
}

/** @brief A procedure to be called by
 * procedures which are called by exiting tasks. */
static void NORETURN do_exit(int arg)
{
	task_t* curr_task = per_core(current_task);
	const uint32_t core_id = CORE_ID;

	kprintf("Terminate task: %u, return value %d\n", curr_task->id, arg);

	page_map_drop();

	// decrease the number of active tasks
	spinlock_irqsave_lock(&readyqueues[core_id].lock);
	readyqueues[core_id].nr_tasks--;
	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	curr_task->status = TASK_FINISHED;
	reschedule();

	kprintf("Kernel panic: scheduler found no valid task\n");
	while(1) {
		HALT;
	}
}

/** @brief A procedure to be called by kernel tasks */
void NORETURN leave_kernel_task(void) {
	int result;

	result = 0; //get_return_value();
	do_exit(result);
}

/** @brief To be called by the systemcall to exit tasks */
void NORETURN sys_exit(int arg) {
	do_exit(arg);
}

/** @brief Aborting a task is like exiting it with result -1 */
void NORETURN abort(void) {
	do_exit(-1);
}

int create_task(tid_t* id, entry_point_t ep, void* arg, uint8_t prio, uint32_t core_id)
{
	int ret = -ENOMEM;
	uint32_t i;

	if (BUILTIN_EXPECT(!ep, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio == IDLE_PRIO, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio > MAX_PRIO, 0))
		return -EINVAL;

	spinlock_irqsave_lock(&table_lock);

	for(i=0; i<MAX_TASKS; i++) {
		if (task_table[i].status == TASK_INVALID) {
			task_table[i].id = i;
			task_table[i].status = TASK_READY;
			task_table[i].last_core = 0;
			task_table[i].last_stack_pointer = NULL;
			task_table[i].stack = create_stack(i);
			task_table[i].prio = prio;
			spinlock_init(&task_table[i].vma_lock);
			task_table[i].vma_list = NULL;
			task_table[i].heap = NULL;

			spinlock_irqsave_init(&task_table[i].page_lock);
			atomic_int32_set(&task_table[i].user_usage, 0);

			/* Allocated new PGD or PML4 and copy page table */
			task_table[i].page_map = get_pages(1);
			if (BUILTIN_EXPECT(!task_table[i].page_map, 0))
				goto out;

			/* Copy page tables & user frames of current task to new one */
			page_map_copy(&task_table[i]);

			if (id)
				*id = i;

			ret = create_default_frame(task_table+i, ep, arg);

			// add task in the readyqueues
			spinlock_irqsave_lock(&readyqueues[core_id].lock);
			readyqueues[core_id].prio_bitmap |= (1 << prio);
			readyqueues[core_id].nr_tasks++;
			if (!readyqueues[core_id].queue[prio-1].first) {
				task_table[i].next = task_table[i].prev = NULL;
				readyqueues[core_id].queue[prio-1].first = task_table+i;
				readyqueues[core_id].queue[prio-1].last = task_table+i;
			} else {
				task_table[i].prev = readyqueues[core_id].queue[prio-1].last;
				task_table[i].next = NULL;
				readyqueues[core_id].queue[prio-1].last->next = task_table+i;
				readyqueues[core_id].queue[prio-1].last = task_table+i;
			}
			spinlock_irqsave_unlock(&readyqueues[core_id].lock);
			break;
		}
	}

out:
	spinlock_irqsave_unlock(&table_lock);

	return ret;
}

int create_kernel_task(tid_t* id, entry_point_t ep, void* args, uint8_t prio)
{
	if (prio > MAX_PRIO)
		prio = NORMAL_PRIO;

	return create_task(id, ep, args, prio, CORE_ID);
}

/** @brief Wakeup a blocked task
 * @param id The task's tid_t structure
 * @return
 * - 0 on success
 * - -EINVAL (-22) on failure
 */
int wakeup_task(tid_t id)
{
	task_t* task;
	uint32_t core_id, prio;
	int ret = -EINVAL;
	uint8_t flags;

	flags = irq_nested_disable();

	task = task_table + id;
	prio = task->prio;
	core_id = task->last_core;

	if (task->status == TASK_BLOCKED) {
		task->status = TASK_READY;
		ret = 0;

		spinlock_irqsave_lock(&readyqueues[core_id].lock);
		// increase the number of ready tasks
		readyqueues[core_id].nr_tasks++;

		// add task to the runqueue
		if (!readyqueues[core_id].queue[prio-1].last) {
			readyqueues[core_id].queue[prio-1].last = readyqueues[core_id].queue[prio-1].first = task;
			task->next = task->prev = NULL;
			readyqueues[core_id].prio_bitmap |= (1 << prio);
		} else {
			task->prev = readyqueues[core_id].queue[prio-1].last;
			task->next = NULL;
			readyqueues[core_id].queue[prio-1].last->next = task;
			readyqueues[core_id].queue[prio-1].last = task;
		}
		spinlock_irqsave_unlock(&readyqueues[core_id].lock);
	}

	irq_nested_enable(flags);

	return ret;
}

/** @brief Block current task
 *
 * The current task's status will be changed to TASK_BLOCKED
 *
 * @return
 * - 0 on success
 * - -EINVAL (-22) on failure
 */
int block_current_task(void)
{
	task_t* curr_task;
	tid_t id;
	uint32_t prio, core_id;
	int ret = -EINVAL;
	uint8_t flags;

	flags = irq_nested_disable();

	curr_task = per_core(current_task);
	id = curr_task->id;
	prio = curr_task->prio;
	core_id = CORE_ID;

	if (task_table[id].status == TASK_RUNNING) {
		task_table[id].status = TASK_BLOCKED;
		ret = 0;

		spinlock_irqsave_lock(&readyqueues[core_id].lock);
		// reduce the number of ready tasks
		readyqueues[core_id].nr_tasks--;

		// remove task from queue
		if (task_table[id].prev)
			task_table[id].prev->next = task_table[id].next;
		if (task_table[id].next)
			task_table[id].next->prev = task_table[id].prev;
		if (readyqueues[core_id].queue[prio-1].first == task_table+id)
			readyqueues[core_id].queue[prio-1].first = task_table[id].next;
		if (readyqueues[core_id].queue[prio-1].last == task_table+id) {
			readyqueues[core_id].queue[prio-1].last = task_table[id].prev;
			if (!readyqueues[core_id].queue[prio-1].last)
				readyqueues[core_id].queue[prio-1].last = readyqueues[core_id].queue[prio-1].first;
		}

		// No valid task in queue => update prio_bitmap
		if (!readyqueues[core_id].queue[prio-1].first)
			readyqueues[core_id].prio_bitmap &= ~(1 << prio);
		spinlock_irqsave_unlock(&readyqueues[core_id].lock);
	}

	irq_nested_enable(flags);

	return ret;
}

size_t** scheduler(void)
{
	task_t* orig_task;
	task_t* curr_task;
	const int32_t core_id = CORE_ID;
	uint32_t prio;

	orig_task = curr_task = per_core(current_task);
	curr_task->last_core = core_id;

	spinlock_irqsave_lock(&readyqueues[core_id].lock);

	/* signalizes that this task could be reused */
	if (curr_task->status == TASK_FINISHED) {
		curr_task->status = TASK_INVALID;
		readyqueues[core_id].old_task = curr_task;
	} else readyqueues[core_id].old_task = NULL; // reset old task

	prio = msb(readyqueues[core_id].prio_bitmap); // determines highest priority
	if (prio > MAX_PRIO) {
		if ((curr_task->status == TASK_RUNNING) || (curr_task->status == TASK_IDLE))
			goto get_task_out;
		curr_task = per_core(current_task) = readyqueues[core_id].idle;
	} else {
		// Does the current task have an higher priority? => no task switch
		if ((curr_task->prio > prio) && (curr_task->status == TASK_RUNNING))
			goto get_task_out;

		if (curr_task->status == TASK_RUNNING) {
			curr_task->status = TASK_READY;
			readyqueues[core_id].old_task = curr_task;
		}

		curr_task = per_core(current_task) = readyqueues[core_id].queue[prio-1].first;
		if (BUILTIN_EXPECT(curr_task->status == TASK_INVALID, 0)) {
			kprintf("Upps!!!!!!! Got invalid task %d, orig task %d\n", curr_task->id, orig_task->id);
		}
		curr_task->status = TASK_RUNNING;

		// remove new task from queue
		// by the way, priority 0 is only used by the idle task and doesn't need own queue
		readyqueues[core_id].queue[prio-1].first = curr_task->next;
		if (!curr_task->next) {
			readyqueues[core_id].queue[prio-1].last = NULL;
			readyqueues[core_id].prio_bitmap &= ~(1 << prio);
		}
		curr_task->next = curr_task->prev = NULL;
	}

get_task_out:
	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	if (curr_task != orig_task) {
		/* if the original task is using the FPU, we need to save the FPU context */
		if ((orig_task->flags & TASK_FPU_USED) && (orig_task->status == TASK_READY)) {
			save_fpu_state(&(orig_task->fpu));
			orig_task->flags &= ~TASK_FPU_USED;
		}

		kprintf("schedule on core %d from %u to %u with prio %u\n", core_id, orig_task->id, curr_task->id, (uint32_t)curr_task->prio);

		return (size_t**) &(orig_task->last_stack_pointer);
	}

	return NULL;
}

void reschedule(void)
{
	size_t** stack;
	uint8_t flags;

	flags = irq_nested_disable();
	if ((stack = scheduler()))
		switch_context(stack);
	irq_nested_enable(flags);
}
