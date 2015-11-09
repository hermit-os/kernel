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
#include <hermit/time.h>
#include <hermit/errno.h>
#include <hermit/syscall.h>
#include <hermit/memory.h>

/** @brief Array of task structures (aka PCB)
 *
 * A task's id will be its position in this array.
 */
static task_t task_table[MAX_TASKS] = { \
		[0]                 = {0, TASK_IDLE, 0, NULL, NULL, TASK_DEFAULT_FLAGS, 0, 0, 0, SPINLOCK_IRQSAVE_INIT, SPINLOCK_INIT, NULL, 0, NULL, NULL, 0, NULL, NULL, 0, 0, 0, -1, 0}, \
		[1 ... MAX_TASKS-1] = {0, TASK_INVALID, 0, NULL, NULL, TASK_DEFAULT_FLAGS, 0, 0, 0, SPINLOCK_IRQSAVE_INIT, SPINLOCK_INIT, NULL, 0, NULL, NULL, 0, NULL, NULL, 0, 0, 0, -1, 0}};

static spinlock_irqsave_t table_lock = SPINLOCK_IRQSAVE_INIT;

#if MAX_CORES > 1
static readyqueues_t readyqueues[MAX_CORES] = { \
		[0 ... MAX_CORES-1]   = {NULL, NULL, 0, 0, 0, {[0 ... MAX_PRIO-2] = {NULL, NULL}}, {NULL, NULL}, SPINLOCK_IRQSAVE_INIT}};
#else
static readyqueues_t readyqueues[1] = {[0] = {task_table+0, NULL, 0, 0, 0, {[0 ... MAX_PRIO-2] = {NULL, NULL}}, {NULL, NULL}, SPINLOCK_IRQSAVE_INIT}};
#endif

DEFINE_PER_CORE(task_t*, current_task, task_table+0);
DEFINE_PER_CORE(char*, kernel_stack, NULL);
#if MAX_CORES > 1
DEFINE_PER_CORE(uint32_t, __core_id, 0);
#endif
extern const void boot_stack;

/** @brief helper function for the assembly code to determine the current task
 * @return Pointer to the task_t structure of current task
 */
task_t* get_current_task(void)
{
	return per_core(current_task);
}

void check_scheduling(void)
{
	if (!is_irq_enabled())
		return;
	if (msb(readyqueues[CORE_ID].prio_bitmap) > per_core(current_task)->prio)
		reschedule();
}

uint32_t get_highest_priority(void)
{
	uint32_t prio = msb(readyqueues[CORE_ID].prio_bitmap);

	if (prio > MAX_PRIO)
		return 0;
	return prio;
}

int multitasking_init(void)
{
	uint32_t core_id = CORE_ID;

	if (BUILTIN_EXPECT(task_table[0].status != TASK_IDLE, 0)) {
		kputs("Task 0 is not an idle task\n");
		return -ENOMEM;
	}

	task_table[0].prio = IDLE_PRIO;
	task_table[0].stack = (char*) ((size_t)&boot_stack + core_id * KERNEL_STACK_SIZE);
	set_per_core(kernel_stack, task_table[0].stack + KERNEL_STACK_SIZE - 0x10);
	set_per_core(current_task, task_table+0);
	task_table[0].page_map = read_cr3();

	readyqueues[core_id].idle = task_table+0;

	return 0;
}

/* interrupt handler to save / restore the FPU context */
void fpu_handler(struct state *s)
{
	task_t* task = per_core(current_task);
	uint32_t core_id = CORE_ID;

	clts(); // clear the TS flag of cr0

	spinlock_irqsave_lock(&readyqueues[core_id].lock);
	// did another already use the the FPU? => save FPU state
	if (readyqueues[core_id].fpu_owner) {
		save_fpu_state(&(task_table[readyqueues[core_id].fpu_owner].fpu));
		readyqueues[core_id].fpu_owner = 0;
	}
	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	if (BUILTIN_EXPECT(!(task->flags & TASK_FPU_INIT), 0))  {
		// use the FPU at the first time => Initialize FPU
 		fpu_init(&task->fpu);
		task->flags |= TASK_FPU_INIT;
	}

	restore_fpu_state(&task->fpu);
	task->flags |= TASK_FPU_USED;
}

int set_idle_task(void)
{
	uint32_t i, core_id = CORE_ID;
	int ret = -ENOMEM;

	spinlock_irqsave_lock(&table_lock);

	for(i=0; i<MAX_TASKS; i++) {
		if (task_table[i].status == TASK_INVALID) {
			task_table[i].id = i;
			task_table[i].status = TASK_IDLE;
			task_table[i].last_core = core_id;
			task_table[i].last_stack_pointer = NULL;
			task_table[i].stack = (char*) ((size_t)&boot_stack + core_id * KERNEL_STACK_SIZE);
			set_per_core(kernel_stack, task_table[i].stack + KERNEL_STACK_SIZE - 0x10);
			task_table[i].prio = IDLE_PRIO;
			spinlock_init(&task_table[i].vma_lock);
			task_table[i].vma_list = NULL;
			task_table[i].heap = NULL;
			spinlock_irqsave_init(&task_table[i].page_lock);
			task_table[i].user_usage = NULL;
			task_table[i].page_map = read_cr3();
			readyqueues[core_id].idle = task_table+i;
			set_per_core(current_task, readyqueues[core_id].idle);
			ret = 0;

			break;
		}
	}

	spinlock_irqsave_unlock(&table_lock);

	return ret;
}

void finish_task_switch(void)
{
	task_t* old;
	uint8_t prio;
	const uint32_t core_id = CORE_ID;

	spinlock_irqsave_lock(&readyqueues[core_id].lock);

	if ((old = readyqueues[core_id].old_task) != NULL) {
		if (old->status == TASK_FINISHED) {
			/* cleanup task */
			if (old->stack) {
				kfree(old->stack);
				old->stack = NULL;
			}

			if (old->user_usage) {
				kfree(old->user_usage);
				old->user_usage = NULL;
			}

			if (!old->parent && old->heap) {
				kfree(old->heap);
				old->heap = NULL;
			}

			old->last_stack_pointer = NULL;
			readyqueues[core_id].old_task = NULL;

			/* signalizes that this task could be reused */
			old->status = TASK_INVALID;
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
}

/** @brief A procedure to be called by
 * procedures which are called by exiting tasks. */
void NORETURN do_exit(int arg)
{
	task_t* curr_task = per_core(current_task);
	const uint32_t core_id = CORE_ID;

	kprintf("Terminate task: %u, return value %d\n", curr_task->id, arg);

	uint8_t flags = irq_nested_disable();

	// Threads should delete the page table and the heap */
	if (!curr_task->parent)
		page_map_drop();

	// decrease the number of active tasks
	spinlock_irqsave_lock(&readyqueues[core_id].lock);
	readyqueues[core_id].nr_tasks--;
	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	curr_task->status = TASK_FINISHED;
	reschedule();

	irq_nested_enable(flags);

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

/** @brief Aborting a task is like exiting it with result -1 */
void NORETURN abort(void) {
	do_exit(-1);
}

uint32_t get_next_core_id(void)
{
	uint32_t i;
	static uint32_t core_id = MAX_CORES;

	if (core_id >= MAX_CORES)
		core_id = CORE_ID;


	// we assume OpenMP applications
	// => number of threads is (normaly) equal to the number of cores
	// => search next available core
	for(i=0, core_id=(core_id+1)%MAX_CORES; i<MAX_CORES; i++, core_id=(core_id+1)%MAX_CORES)
		if (readyqueues[core_id].idle)
			break;

	return core_id;
}

int clone_task(tid_t* id, entry_point_t ep, void* arg, uint8_t prio)
{
	int ret = -EINVAL;
	uint32_t i;
	void* stack = NULL;
	task_t* curr_task;
	uint32_t core_id;

	if (BUILTIN_EXPECT(!ep, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio == IDLE_PRIO, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio > MAX_PRIO, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT((size_t)ep < KERNEL_SPACE, 0))
		return -EINVAL;

	curr_task = per_core(current_task);

	stack = kmalloc(KERNEL_STACK_SIZE);
	if (BUILTIN_EXPECT(!stack, 0))
		return -ENOMEM;

	spinlock_irqsave_lock(&table_lock);

	core_id = get_next_core_id();
	if ((core_id >= MAX_CORES) || !readyqueues[core_id].idle)
		core_id = CORE_ID;

	kprintf("start new thread on core %d\n", core_id);

	for(i=0; i<MAX_TASKS; i++) {
		if (task_table[i].status == TASK_INVALID) {
			task_table[i].id = i;
			task_table[i].status = TASK_READY;
			task_table[i].last_core = 0;
			task_table[i].last_stack_pointer = NULL;
			task_table[i].stack = stack;
			task_table[i].prio = prio;
			task_table[i].vma_list = curr_task->vma_list;
			task_table[i].heap = curr_task->heap;
                        task_table[i].start_tick = get_clock_tick();
			task_table[i].parent = curr_task->id;
			task_table[i].tls_addr = curr_task->tls_addr;
			task_table[i].tls_mem_size = curr_task->tls_mem_size;
			task_table[i].tls_file_size = curr_task->tls_file_size;
			task_table[i].sd = task_table[i].sd;
			task_table[i].lwip_err = 0;
			task_table[i].user_usage = curr_task->user_usage;
			task_table[i].page_map = curr_task->page_map;

			if (id)
				*id = i;

			ret = create_default_frame(task_table+i, ep, arg, core_id);
			if (ret)
				goto out;

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

	spinlock_irqsave_unlock(&table_lock);
out:
	if (ret)
		kfree(stack);

	if (core_id != CORE_ID)
		apic_send_ipi(core_id, 121);

	return ret;
}

int create_task(tid_t* id, entry_point_t ep, void* arg, uint8_t prio, uint32_t core_id)
{
	int ret = -ENOMEM;
	uint32_t i;
	void* stack = NULL;
	void* counter = NULL;

	if (BUILTIN_EXPECT(!ep, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio == IDLE_PRIO, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio > MAX_PRIO, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(core_id >= MAX_CORES, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(!readyqueues[core_id].idle, 0))
		return -EINVAL;

	stack = kmalloc(KERNEL_STACK_SIZE);
	if (BUILTIN_EXPECT(!stack, 0))
		return -ENOMEM;
	counter = kmalloc(sizeof(atomic_int64_t));
	if (BUILTIN_EXPECT(!counter, 0)) {
		kfree(stack);
		return -ENOMEM;
	}
	atomic_int64_set((atomic_int64_t*) counter, 0);

	spinlock_irqsave_lock(&table_lock);

	for(i=0; i<MAX_TASKS; i++) {
		if (task_table[i].status == TASK_INVALID) {
			task_table[i].id = i;
			task_table[i].status = TASK_READY;
			task_table[i].last_core = 0;
			task_table[i].last_stack_pointer = NULL;
			task_table[i].stack = stack;
			task_table[i].prio = prio;
			spinlock_init(&task_table[i].vma_lock);
			task_table[i].vma_list = NULL;
			task_table[i].heap = NULL;
			task_table[i].start_tick = get_clock_tick();
			task_table[i].parent = 0;
			task_table[i].tls_addr = 0;
			task_table[i].tls_mem_size = 0;
			task_table[i].tls_file_size = 0;
			task_table[i].sd = -1;
			task_table[i].lwip_err = 0;

			spinlock_irqsave_init(&task_table[i].page_lock);
			task_table[i].user_usage = (atomic_int64_t*) counter;

			/* Allocated new PGD or PML4 and copy page table */
			task_table[i].page_map = get_pages(1);
			if (BUILTIN_EXPECT(!task_table[i].page_map, 0))
				goto out;

			/* Copy page tables & user frames of current task to new one */
			page_map_copy(&task_table[i]);

			if (id)
				*id = i;
			//kprintf("Create task %d with pml4 at 0x%llx\n", i, task_table[i].page_map);

			ret = create_default_frame(task_table+i, ep, arg, core_id);
			if (ret)
				goto out;

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

	if (ret) {
		kfree(stack);
		kfree(counter);
	}

	if (core_id != CORE_ID)
		apic_send_ipi(core_id, 121);

	return ret;
}

int create_user_task(tid_t* id, const char* fname, char** argv, uint8_t prio)
{
	if (prio > MAX_PRIO)
		prio = NORMAL_PRIO;

	return create_user_task_on_core(id, fname, argv, prio, CORE_ID);
}

int create_kernel_task_on_core(tid_t* id, entry_point_t ep, void* args, uint8_t prio, uint32_t core_id)
{
	if (prio > MAX_PRIO)
		prio = NORMAL_PRIO;

	return create_task(id, ep, args, prio, core_id);
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

		// do we need to remove from timer queue?
		if (task->flags & TASK_TIMER) {
			task->flags &= ~TASK_TIMER;
			if (task->prev)
				task->prev->next = task->next;
			if (task->next)
				task->next->prev = task->prev;
			if (readyqueues[core_id].timers.first == task)
				readyqueues[core_id].timers.first = task->next;
			if (readyqueues[core_id].timers.last == task)
				readyqueues[core_id].timers.last = task->prev;
		}

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

#ifdef DYNAMIC_TICKS
		// send IPI to be sure that the scheuler recognize the new task
		if (core_id != CORE_ID)
			apic_send_ipi(core_id, 121);
#endif
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

int set_timer(uint64_t deadline)
{
	task_t* curr_task;
	task_t* tmp;
	uint32_t core_id, prio;
	uint32_t flags;
	int ret = -EINVAL;

	flags = irq_nested_disable();

	curr_task = per_core(current_task);
	prio = curr_task->prio;
	core_id = CORE_ID;

	if (curr_task->status == TASK_RUNNING) {
		curr_task->status = TASK_BLOCKED;
		curr_task->timeout = deadline;
		curr_task->flags |= TASK_TIMER;
		ret = 0;

		spinlock_irqsave_lock(&readyqueues[core_id].lock);

		// reduce the number of ready tasks
		readyqueues[core_id].nr_tasks--;

		// remove task from queue
		if (curr_task->prev)
			curr_task->prev->next = curr_task->next;
		if (curr_task->next)
			curr_task->next->prev = curr_task->prev;
		if (readyqueues[core_id].queue[prio-1].first == curr_task)
			readyqueues[core_id].queue[prio-1].first = curr_task->next;
		if (readyqueues[core_id].queue[prio-1].last == curr_task) {
			readyqueues[core_id].queue[prio-1].last = curr_task->prev;
			if (!readyqueues[core_id].queue[prio-1].last)
				readyqueues[core_id].queue[prio-1].last = readyqueues[core_id].queue[prio-1].first;
		}

		// No valid task in queue => update prio_bitmap
		if (!readyqueues[core_id].queue[prio-1].first)
			readyqueues[core_id].prio_bitmap &= ~(1 << prio);

		// add task to the timer queue
		tmp = readyqueues[core_id].timers.first;
		if (!tmp) {
			readyqueues[core_id].timers.first = readyqueues[core_id].timers.last = curr_task;
			curr_task->prev = curr_task->next = NULL;
#ifdef DYNAMIC_TICKS
			timer_deadline(deadline-get_clock_tick());
#endif
		} else {
			while(tmp && (deadline >= tmp->timeout))
				tmp = tmp->next;

			if (!tmp) {
				curr_task->next = NULL;
				curr_task->prev = readyqueues[core_id].timers.last;
				if (readyqueues[core_id].timers.last)
					readyqueues[core_id].timers.last->next = curr_task;
				readyqueues[core_id].timers.last = curr_task;
				// obsolete lines...
				//if (!readyqueues[core_id].timers.first)
				//      readyqueues[core_id].timers.first = curr_task;
			} else {
				curr_task->prev = tmp->prev;
				curr_task->next = tmp;
				tmp->prev = curr_task;
				if (curr_task->prev)
					curr_task->prev->next = curr_task;
				if (readyqueues[core_id].timers.first == tmp) {
					readyqueues[core_id].timers.first = curr_task;
#ifdef DYNAMIC_TICKS
					timer_deadline(deadline-get_clock_tick());
#endif
				}
			}
		}

		spinlock_irqsave_unlock(&readyqueues[core_id].lock);
	} else kprintf("Task is already blocked. No timer will be set!\n");

	irq_nested_enable(flags);

	return ret;
}

void check_timers(void)
{
	uint32_t core_id = CORE_ID;
	uint32_t prio;
	uint64_t current_tick;

	spinlock_irqsave_lock(&readyqueues[core_id].lock);

        // check timers
	current_tick = get_clock_tick();
	while (readyqueues[core_id].timers.first && readyqueues[core_id].timers.first->timeout <= current_tick)
	{
		task_t* task = readyqueues[core_id].timers.first;

		// remove timer from queue
		readyqueues[core_id].timers.first = readyqueues[core_id].timers.first->next;
		if (readyqueues[core_id].timers.first) {
			readyqueues[core_id].timers.first->prev = NULL;
#ifdef DYNAMIC_TICKS
			if (readyqueues[core_id].timers.first->timeout > get_clock_tick())
				timer_deadline(readyqueues[core_id].timers.first->timeout-current_tick);
#endif
		} else  readyqueues[core_id].timers.last = NULL;
		task->flags &= ~TASK_TIMER;

		// wakeup task
		if (task->status == TASK_BLOCKED) {
			task->status = TASK_READY;
			prio = task->prio;

			// increase the number of ready tasks
			readyqueues[core_id].nr_tasks++;

			// add task to the runqueue
			if (!readyqueues[core_id].queue[prio-1].first) {
				readyqueues[core_id].queue[prio-1].last = readyqueues[core_id].queue[prio-1].first = task;
				task->next = task->prev = NULL;
				readyqueues[core_id].prio_bitmap |= (1 << prio);
			} else {
				task->prev = readyqueues[core_id].queue[prio-1].last;
				task->next = NULL;
				readyqueues[core_id].queue[prio-1].last->next = task;
				readyqueues[core_id].queue[prio-1].last = task;
			}
		}
	}

	spinlock_irqsave_unlock(&readyqueues[core_id].lock);
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

	/* signalizes that this task could be realized */
	if (curr_task->status == TASK_FINISHED)
		readyqueues[core_id].old_task = curr_task;
	else readyqueues[core_id].old_task = NULL; // reset old task

	prio = msb(readyqueues[core_id].prio_bitmap); // determines highest priority
	if (prio > MAX_PRIO) {
		if ((curr_task->status == TASK_RUNNING) || (curr_task->status == TASK_IDLE))
			goto get_task_out;
		curr_task = readyqueues[core_id].idle;
		set_per_core(current_task, curr_task);
	} else {
		// Does the current task have an higher priority? => no task switch
		if ((curr_task->prio > prio) && (curr_task->status == TASK_RUNNING))
			goto get_task_out;

		if (curr_task->status == TASK_RUNNING) {
			curr_task->status = TASK_READY;
			readyqueues[core_id].old_task = curr_task;
		}

		curr_task = readyqueues[core_id].queue[prio-1].first;
		set_per_core(current_task, curr_task);
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
	if (curr_task != orig_task) {
		/* if the original task is using the FPU, we need to save the FPU context */
		if ((orig_task->flags & TASK_FPU_USED) && (orig_task->status == TASK_READY)) {
			readyqueues[core_id].fpu_owner = orig_task->id;
			orig_task->flags &= ~TASK_FPU_USED;
		}

		spinlock_irqsave_unlock(&readyqueues[core_id].lock);

		//kprintf("schedule on core %d from %u to %u with prio %u\n", core_id, orig_task->id, curr_task->id, (uint32_t)curr_task->prio);

		return (size_t**) &(orig_task->last_stack_pointer);
	} else spinlock_irqsave_unlock(&readyqueues[core_id].lock);

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
