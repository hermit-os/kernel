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
#include <hermit/logging.h>
#include <asm/processor.h>

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern atomic_int32_t cpu_online;

volatile uint32_t go_down = 0;

#define TLS_OFFSET	8

/** @brief Array of task structures (aka PCB)
 *
 * A task's id will be its position in this array.
 */
static task_t task_table[MAX_TASKS] = { \
        [0]                 = {0, TASK_IDLE, 0, NULL, NULL, NULL, TASK_DEFAULT_FLAGS, 0, 0, 0, 0, NULL, 0, NULL, NULL, 0, 0, 0, NULL, FPU_STATE_INIT}, \
        [1 ... MAX_TASKS-1] = {0, TASK_INVALID, 0, NULL, NULL, NULL, TASK_DEFAULT_FLAGS, 0, 0, 0, 0, NULL, 0, NULL, NULL, 0, 0, 0, NULL, FPU_STATE_INIT}};

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
extern const void boot_ist;


static void update_timer(task_t* first)
{
	if(first) {
		if(first->timeout > get_clock_tick()) {
			timer_deadline((uint32_t) (first->timeout - get_clock_tick()));
		} else {
			// workaround: start timer so new head will be serviced
			timer_deadline(1);
		}
	} else {
		// prevent spurious interrupts
		timer_disable();
	}
}


static void timer_queue_remove(uint32_t core_id, task_t* task)
{
	if(BUILTIN_EXPECT(!task, 0)) {
		return;
	}

	task_list_t* timer_queue = &readyqueues[core_id].timers;

#ifdef DYNAMIC_TICKS
	// if task is first in timer queue, we need to update the oneshot
	// timer for the next task
	if(timer_queue->first == task) {
		update_timer(task->next);
	}
#endif

	task_list_remove_task(timer_queue, task);
}


static void timer_queue_push(uint32_t core_id, task_t* task)
{
	task_list_t* timer_queue = &readyqueues[core_id].timers;

	spinlock_irqsave_lock(&readyqueues[core_id].lock);

	task_t* first = timer_queue->first;

	if(!first) {
		timer_queue->first = timer_queue->last = task;
		task->next = task->prev = NULL;

        #ifdef DYNAMIC_TICKS
		    update_timer(task);
        #endif
	} else {
		// lookup position where to insert task
		task_t* tmp = first;
		while(tmp && (task->timeout >= tmp->timeout))
			tmp = tmp->next;

		if(!tmp) {
			// insert at the end of queue
			task->next = NULL;
			task->prev = timer_queue->last;

			// there has to be a last element because there is also a first one
			timer_queue->last->next = task;
			timer_queue->last = task;
		} else {
			task->next = tmp;
			task->prev = tmp->prev;
			tmp->prev = task;

			if(task->prev)
				task->prev->next = task;

			if(timer_queue->first == tmp) {
				timer_queue->first = task;

                #ifdef DYNAMIC_TICKS
				    update_timer(task);
                #endif
			}
		}
	}

	spinlock_irqsave_unlock(&readyqueues[core_id].lock);
}


static void readyqueues_push_back(uint32_t core_id, task_t* task)
{
	// idle task (prio=0) doesn't have a queue
	task_list_t* readyqueue = &readyqueues[core_id].queue[task->prio - 1];

	task_list_push_back(readyqueue, task);

	// update priority bitmap
	readyqueues[core_id].prio_bitmap |= (1 << task->prio);

	// increase the number of ready tasks
	readyqueues[core_id].nr_tasks++;
}


static void readyqueues_remove(uint32_t core_id, task_t* task)
{
	// idle task (prio=0) doesn't have a queue
	task_list_t* readyqueue = &readyqueues[core_id].queue[task->prio - 1];

	task_list_remove_task(readyqueue, task);

	// no valid task in queue => update priority bitmap
	if (readyqueue->first == NULL)
		readyqueues[core_id].prio_bitmap &= ~(1 << task->prio);

	// reduce the number of ready tasks
	readyqueues[core_id].nr_tasks--;
}


void fpu_handler(void)
{
	task_t* task = per_core(current_task);
	uint32_t core_id = CORE_ID;

	task->flags |= TASK_FPU_USED;

	if (!(task->flags & TASK_FPU_INIT))  {
		// use the FPU at the first time => Initialize FPU
		fpu_init(&task->fpu);
		task->flags |= TASK_FPU_INIT;
	}

	if (readyqueues[core_id].fpu_owner == task->id)
		return;

	spinlock_irqsave_lock(&readyqueues[core_id].lock);
	// did another already use the the FPU? => save FPU state
	if (readyqueues[core_id].fpu_owner) {
		save_fpu_state(&(task_table[readyqueues[core_id].fpu_owner].fpu));
		task_table[readyqueues[core_id].fpu_owner].flags &= ~TASK_FPU_USED;
	}
	readyqueues[core_id].fpu_owner = task->id;
	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	restore_fpu_state(&task->fpu);
}

void check_scheduling(void)
{
	if (!is_irq_enabled())
		return;

	uint32_t prio = get_highest_priority();
	task_t* curr_task = per_core(current_task);

	if (prio > curr_task->prio) {
		reschedule();
#ifdef DYNAMIC_TICKS
	} else if ((prio > 0) && (prio == curr_task->prio)) {
		// if a task is ready, check if the current task runs already one tick (one time slice)
		// => reschedule to realize round robin

		const uint64_t diff_cycles = get_rdtsc() - curr_task->last_tsc;
		const uint64_t cpu_freq_hz = 1000000ULL * (uint64_t) get_cpu_frequency();

		if (((diff_cycles * (uint64_t) TIMER_FREQ) / cpu_freq_hz) > 0) {
			LOG_DEBUG("Time slice expired for task %d on core %d. New task has priority %u.\n", curr_task->id, CORE_ID, prio);
			reschedule();
		}
#endif
	}
}


uint32_t get_highest_priority(void)
{
	uint32_t prio = msb(readyqueues[CORE_ID].prio_bitmap);

	if (prio > MAX_PRIO)
		return 0;
	return prio;
}


void* get_readyqueue(void)
{
	return &readyqueues[CORE_ID];
}


int multitasking_init(void)
{
	uint32_t core_id = CORE_ID;

	if (BUILTIN_EXPECT(task_table[0].status != TASK_IDLE, 0)) {
		LOG_ERROR("Task 0 is not an idle task\n");
		return -ENOMEM;
	}

	task_table[0].prio = IDLE_PRIO;
	task_table[0].stack = (char*) ((size_t)&boot_stack + core_id * KERNEL_STACK_SIZE);
	task_table[0].ist_addr = (char*)&boot_ist;
	set_per_core(kernel_stack, task_table[0].stack + KERNEL_STACK_SIZE - 0x10);
	set_per_core(current_task, task_table+0);
	arch_init_task(task_table+0);

	readyqueues[core_id].idle = task_table+0;

	return 0;
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
			task_table[i].ist_addr = create_stack(KERNEL_STACK_SIZE);
			set_per_core(kernel_stack, task_table[i].stack + KERNEL_STACK_SIZE - 0x10);
			task_table[i].prio = IDLE_PRIO;
			task_table[i].heap = NULL;
			readyqueues[core_id].idle = task_table+i;
			set_per_core(current_task, readyqueues[core_id].idle);
			arch_init_task(task_table+i);
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
	const uint32_t core_id = CORE_ID;

	spinlock_irqsave_lock(&readyqueues[core_id].lock);

	if ((old = readyqueues[core_id].old_task) != NULL) {
		readyqueues[core_id].old_task = NULL;

		if (old->status == TASK_FINISHED) {
			/* cleanup task */
			if (old->stack) {
				LOG_INFO("Release stack at 0x%zx\n", old->stack);
				destroy_stack(old->stack, DEFAULT_STACK_SIZE);
				old->stack = NULL;
			}

			if (!old->parent && old->heap) {
				kfree(old->heap);
				old->heap = NULL;
			}

			if (old->ist_addr) {
				destroy_stack(old->ist_addr, KERNEL_STACK_SIZE);
				old->ist_addr = NULL;
			}

			old->last_stack_pointer = NULL;

			if (readyqueues[core_id].fpu_owner == old->id)
				readyqueues[core_id].fpu_owner = 0;

			/* signalizes that this task could be reused */
			old->status = TASK_INVALID;
		} else {
			// re-enqueue old task
			readyqueues_push_back(core_id, old);
		}
	}

	spinlock_irqsave_unlock(&readyqueues[core_id].lock);
}


void NORETURN do_exit(int arg)
{
	task_t* curr_task = per_core(current_task);
	void* tls_addr = NULL;
	const uint32_t core_id = CORE_ID;

	LOG_INFO("Terminate task: %u, return value %d\n", curr_task->id, arg);

	uint8_t flags = irq_nested_disable();

	// decrease the number of active tasks
	spinlock_irqsave_lock(&readyqueues[core_id].lock);
	readyqueues[core_id].nr_tasks--;
	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	// do we need to release the TLS?
	tls_addr = (void*)get_tls();
	if (tls_addr) {
		LOG_INFO("Release TLS at %p\n", (char*)tls_addr - curr_task->tls_size);
		kfree((char*)tls_addr - curr_task->tls_size - TLS_OFFSET);
	}

	curr_task->status = TASK_FINISHED;
	reschedule();

	irq_nested_enable(flags);

	LOG_ERROR("Kernel panic: scheduler found no valid task\n");
	while(1) {
		HALT;
	}
}


void NORETURN leave_kernel_task(void) {
	int result;

	result = 0; //get_return_value();
	do_exit(result);
}


void NORETURN do_abort(void) {
	do_exit(-1);
}


static uint32_t get_next_core_id(void)
{
	uint32_t i;
	static uint32_t core_id = MAX_CORES;

	if (core_id >= MAX_CORES)
		core_id = CORE_ID;

	// we assume OpenMP applications
	// => number of threads is (normaly) equal to the number of cores
	// => search next available core
	for(i=0, core_id=(core_id+1)%MAX_CORES; i<2*MAX_CORES; i++, core_id=(core_id+1)%MAX_CORES)
		if (readyqueues[core_id].idle)
			break;

	if (BUILTIN_EXPECT(!readyqueues[core_id].idle, 0)) {
		LOG_ERROR("BUG: no core available!\n");
		return MAX_CORES;
	}

	return core_id;
}


int clone_task(tid_t* id, entry_point_t ep, void* arg, uint8_t prio)
{
	int ret = -EINVAL;
	uint32_t i;
	void* stack = NULL;
	void* ist = NULL;
	task_t* curr_task;
	uint32_t core_id;

	if (BUILTIN_EXPECT(!ep, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio == IDLE_PRIO, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(prio > MAX_PRIO, 0))
		return -EINVAL;

	curr_task = per_core(current_task);

	stack = create_stack(DEFAULT_STACK_SIZE);
	if (BUILTIN_EXPECT(!stack, 0))
		return -ENOMEM;

	ist =  create_stack(KERNEL_STACK_SIZE);
	if (BUILTIN_EXPECT(!ist, 0)) {
		destroy_stack(stack, DEFAULT_STACK_SIZE);
		return -ENOMEM;
	}

	spinlock_irqsave_lock(&table_lock);

	core_id = get_next_core_id();
	if (BUILTIN_EXPECT(core_id >= MAX_CORES, 0))
	{
		spinlock_irqsave_unlock(&table_lock);
		ret = -EINVAL;
		goto out;
	}

	for(i=0; i<MAX_TASKS; i++) {
		if (task_table[i].status == TASK_INVALID) {
			task_table[i].id = i;
			task_table[i].status = TASK_READY;
			task_table[i].last_core = core_id;
			task_table[i].last_stack_pointer = NULL;
			task_table[i].stack = stack;
			task_table[i].prio = prio;
			task_table[i].heap = curr_task->heap;
                        task_table[i].start_tick = get_clock_tick();
			task_table[i].last_tsc = 0;
			task_table[i].parent = curr_task->id;
			task_table[i].tls_addr = curr_task->tls_addr;
			task_table[i].tls_size = curr_task->tls_size;
			task_table[i].ist_addr = ist;
			task_table[i].lwip_err = 0;
			task_table[i].signal_handler = NULL;

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

	if (!ret) {
		LOG_DEBUG("start new thread %d on core %d with stack address %p\n", i, core_id, stack);
	}

out:
	if (ret) {
		destroy_stack(stack, DEFAULT_STACK_SIZE);
		destroy_stack(ist, KERNEL_STACK_SIZE);
	}

#if 0
	if (core_id != CORE_ID)
		apic_send_ipi(core_id, 121);
#endif

	return ret;
}


int create_task(tid_t* id, entry_point_t ep, void* arg, uint8_t prio, uint32_t core_id)
{
	int ret = -ENOMEM;
	uint32_t i;
	void* stack = NULL;
	void* ist = NULL;
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

	stack = create_stack(DEFAULT_STACK_SIZE);
	if (BUILTIN_EXPECT(!stack, 0))
		return -ENOMEM;

	ist = create_stack(KERNEL_STACK_SIZE);
	if (BUILTIN_EXPECT(!ist, 0)) {
		destroy_stack(stack, DEFAULT_STACK_SIZE);
		return -ENOMEM;
	}

	counter = kmalloc(sizeof(atomic_int64_t));
	if (BUILTIN_EXPECT(!counter, 0)) {
		destroy_stack(stack, KERNEL_STACK_SIZE);
		destroy_stack(stack, DEFAULT_STACK_SIZE);
		return -ENOMEM;
	}
	atomic_int64_set((atomic_int64_t*) counter, 0);

	spinlock_irqsave_lock(&table_lock);

	for(i=0; i<MAX_TASKS; i++) {
		if (task_table[i].status == TASK_INVALID) {
			task_table[i].id = i;
			task_table[i].status = TASK_READY;
			task_table[i].last_core = core_id;
			task_table[i].last_stack_pointer = NULL;
			task_table[i].stack = stack;
			task_table[i].prio = prio;
			task_table[i].heap = NULL;
			task_table[i].start_tick = get_clock_tick();
			task_table[i].last_tsc = 0;
			task_table[i].parent = 0;
			task_table[i].ist_addr = ist;
			task_table[i].tls_addr = 0;
			task_table[i].tls_size = 0;
			task_table[i].lwip_err = 0;
			task_table[i].signal_handler = NULL;

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

	if (!ret)
		LOG_INFO("start new task %d on core %d with stack address %p\n", i, core_id, stack);

out:
	spinlock_irqsave_unlock(&table_lock);

	if (ret) {
		destroy_stack(stack, DEFAULT_STACK_SIZE);
		destroy_stack(ist, KERNEL_STACK_SIZE);
		kfree(counter);
	}

#if 0
	if (core_id != CORE_ID)
		apic_send_ipi(core_id, 121);
#endif

	return ret;
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


int wakeup_task(tid_t id)
{
	task_t* task;
	uint32_t core_id;
	int ret = -EINVAL;
	uint8_t flags;

	flags = irq_nested_disable();

	task = &task_table[id];
	core_id = task->last_core;

	if (task->status == TASK_BLOCKED) {
		LOG_DEBUG("wakeup task %d\n", id);

		task->status = TASK_READY;
		ret = 0;

		spinlock_irqsave_lock(&readyqueues[core_id].lock);

		// if task is in timer queue, remove it
		if (task->flags & TASK_TIMER) {
			task->flags &= ~TASK_TIMER;

			timer_queue_remove(core_id, task);
		}

		// add task to the ready queue
		readyqueues_push_back(core_id, task);

		spinlock_irqsave_unlock(&readyqueues[core_id].lock);
	}

	irq_nested_enable(flags);

	return ret;
}


int block_task(tid_t id)
{
	task_t* task;
	uint32_t core_id;
	int ret = -EINVAL;
	uint8_t flags;

	flags = irq_nested_disable();

	task = &task_table[id];
	core_id = task->last_core;

	if (task->status == TASK_RUNNING) {
		LOG_DEBUG("block task %d\n", id);

		task->status = TASK_BLOCKED;

		spinlock_irqsave_lock(&readyqueues[core_id].lock);

		// remove task from ready queue
		readyqueues_remove(core_id, task);

		spinlock_irqsave_unlock(&readyqueues[core_id].lock);

		ret = 0;
	}

	irq_nested_enable(flags);

	return ret;
}


int block_current_task(void)
{
	return block_task(per_core(current_task)->id);
}


int set_timer(uint64_t deadline)
{
	task_t* curr_task;
	uint32_t core_id;
	uint8_t flags;
	int ret = -EINVAL;

	flags = irq_nested_disable();

	curr_task = per_core(current_task);
	core_id = CORE_ID;

	if (curr_task->status == TASK_RUNNING) {
		// blocks task and removes from ready queue
		block_task(curr_task->id);

		curr_task->flags |= TASK_TIMER;
		curr_task->timeout = deadline;

		timer_queue_push(core_id, curr_task);

		ret = 0;
	} else {
		LOG_INFO("Task is already blocked. No timer will be set!\n");
	}

	irq_nested_enable(flags);

	return ret;
}


void check_timers(void)
{
	readyqueues_t* readyqueue = &readyqueues[CORE_ID];
	spinlock_irqsave_lock(&readyqueue->lock);

	// since IRQs are disabled, get_clock_tick() won't increase here
	const uint64_t current_tick = get_clock_tick();

	// wakeup tasks whose deadline has expired
	task_t* task;
	while ((task = readyqueue->timers.first) && (task->timeout <= current_tick))
	{
		// pops task from timer queue, so next iteration has new first element
		wakeup_task(task->id);
	}

	spinlock_irqsave_unlock(&readyqueue->lock);
}


size_t** scheduler(void)
{
	task_t* orig_task;
	task_t* curr_task;
	const uint32_t core_id = CORE_ID;
	uint64_t prio;

	orig_task = curr_task = per_core(current_task);
	curr_task->last_core = core_id;

	spinlock_irqsave_lock(&readyqueues[core_id].lock);

	/* signalizes that this task could be realized */
	if (curr_task->status == TASK_FINISHED)
		readyqueues[core_id].old_task = curr_task;
	else readyqueues[core_id].old_task = NULL; // reset old task

	// do we receive a shutdown IPI => only the idle task should get the core
	if (BUILTIN_EXPECT(go_down, 0)) {
		if (curr_task->status == TASK_IDLE)
			goto get_task_out;
		curr_task = readyqueues[core_id].idle;
		set_per_core(current_task, curr_task);
	}

	// determine highest priority
	prio = msb(readyqueues[core_id].prio_bitmap);

	const int readyqueue_empty = prio > MAX_PRIO;
	if (readyqueue_empty) {

		if ((curr_task->status == TASK_RUNNING) || (curr_task->status == TASK_IDLE))
			goto get_task_out;
		curr_task = readyqueues[core_id].idle;
		set_per_core(current_task, curr_task);
	} else {
		// Does the current task have an higher priority? => no task switch
		if ((curr_task->prio > prio) && (curr_task->status == TASK_RUNNING))
			goto get_task_out;

		// mark current task for later cleanup by finish_task_switch()
		if (curr_task->status == TASK_RUNNING) {
			curr_task->status = TASK_READY;
			readyqueues[core_id].old_task = curr_task;
		}

		// get new task from its ready queue
		curr_task = task_list_pop_front(&readyqueues[core_id].queue[prio-1]);

		if(BUILTIN_EXPECT(curr_task == NULL, 0)) {
			LOG_ERROR("Kernel panic: No task in readyqueue\n");
			while(1);
		}
		if (BUILTIN_EXPECT(curr_task->status == TASK_INVALID, 0)) {
			LOG_ERROR("Kernel panic: Got invalid task %d, orig task %d\n",
			        curr_task->id, orig_task->id);
			while(1);
		}

		// if we removed the last task from queue, update priority bitmap
		if(readyqueues[core_id].queue[prio-1].first == NULL) {
			readyqueues[core_id].prio_bitmap &= ~(1 << prio);
		}

		// finally make it the new current task
		curr_task->status = TASK_RUNNING;
#ifdef DYNAMIC_TICKS
		curr_task->last_tsc = get_rdtsc();
#endif
		set_per_core(current_task, curr_task);
	}

get_task_out:
	spinlock_irqsave_unlock(&readyqueues[core_id].lock);

	if (curr_task != orig_task) {
		LOG_DEBUG("schedule on core %d from %u to %u with prio %u\n", core_id, orig_task->id, curr_task->id, (uint32_t)curr_task->prio);

		return (size_t**) &(orig_task->last_stack_pointer);
	}

	return NULL;
}


int get_task(tid_t id, task_t** task)
{
	if (BUILTIN_EXPECT(task == NULL, 0)) {
		return -ENOMEM;
	}

	if (BUILTIN_EXPECT(id >= MAX_TASKS, 0)) {
		return -ENOENT;
	}

	if (BUILTIN_EXPECT(task_table[id].status == TASK_INVALID, 0)) {
		return -EINVAL;
	}

	*task = &task_table[id];

	return 0;
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
