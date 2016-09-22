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
 * @file include/hermit/tasks_types.h
 * @brief Task related structure definitions
 *
 * This file contains the task_t structure definition 
 * and task state define constants
 */

#ifndef __TASKS_TYPES_H__
#define __TASKS_TYPES_H__

#include <hermit/stddef.h>
#include <hermit/spinlock_types.h>
#include <hermit/vma.h>
#include <hermit/signal.h>
#include <asm/tasks_types.h>
#include <asm/atomic.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TASK_INVALID	0
#define TASK_READY	1
#define TASK_RUNNING	2
#define TASK_BLOCKED	3
#define TASK_FINISHED	4
#define TASK_IDLE	5

#define TASK_DEFAULT_FLAGS	0
#define TASK_FPU_INIT		(1 << 0)
#define TASK_FPU_USED		(1 << 1)
#define TASK_TIMER		(1 << 2)

#define MAX_PRIO	31
#define REALTIME_PRIO	31
#define HIGH_PRIO	16
#define NORMAL_PRIO	8
#define LOW_PRIO	1
#define IDLE_PRIO	0

typedef int (*entry_point_t)(void*);

/** @brief Represents a the process control block */
typedef struct task {
	/// Task id = position in the task table
	tid_t			id __attribute__ ((aligned (CACHE_LINE)));
	/// Task status (INVALID, READY, RUNNING, ...)
	uint32_t		status;
	/// last core id on which the task was running
	uint32_t		last_core;
	/// copy of the stack pointer before a context switch
	size_t*			last_stack_pointer;
	/// start address of the stack 
	void*			stack;
	/// interrupt stack for IST1
	void*			ist_addr;
	/// Additional status flags. For instance, to signalize the using of the FPU
	uint8_t			flags;
	/// Task priority
	uint8_t			prio;
	/// timeout for a blocked task
	uint64_t		timeout;
	/// starting time/tick of the task
	uint64_t		start_tick;
	/// last TSC, when the task got the CPU
	uint64_t		last_tsc;
	/// the userspace heap
	vma_t*			heap;
	/// parent thread
	tid_t			parent;
	/// next task in the queue
	struct task*	next;
	/// previous task in the queue
	struct task*	prev;
	/// TLS address
	size_t		tls_addr;
	/// TLS file size
	size_t		tls_size;
	/// LwIP error code
	int		lwip_err;
	/// Handler for (POSIX) Signals
	signal_handler_t signal_handler;
	/// FPU state
	union fpu_state	fpu;
} task_t;

typedef struct {
        task_t* first;
        task_t* last;
} task_list_t;

/** @brief Represents a queue for all runable tasks */
typedef struct {
	/// idle task
	task_t*		idle __attribute__ ((aligned (CACHE_LINE)));
        /// previous task
	task_t*		old_task;
	/// last task, which used the FPU
	tid_t		fpu_owner;
	/// total number of tasks in the queue
	uint32_t	nr_tasks;
	/// indicates the used priority queues
	uint32_t	prio_bitmap;
	/// a queue for each priority
	task_list_t	queue[MAX_PRIO];
	/// a queue for timers
	task_list_t     timers;
	/// lock for this runqueue
	spinlock_irqsave_t lock;
} readyqueues_t;


static inline void task_list_remove_task(task_list_t* list, task_t* task)
{
	if (task->prev)
		task->prev->next = task->next;

	if (task->next)
		task->next->prev = task->prev;

	if (list->last == task)
		list->last = task->prev;

	if (list->first == task)
		list->first = task->next;
}


static inline void task_list_push_back(task_list_t* list, task_t* task)
{
	if(BUILTIN_EXPECT((task == NULL) || (list == NULL), 0)) {
		return;
	}

	if (list->last) {
		task->prev = list->last;
		task->next = NULL;
		list->last->next = task;
		list->last = task;
	} else {
		list->last = list->first = task;
		task->next = task->prev = NULL;
	}
}


static inline task_t* task_list_pop_front(task_list_t* list)
{
	if(BUILTIN_EXPECT((list == NULL), 0)) {
		return NULL;
	}

	task_t* task = list->first;

	if(list->first) {
		// advance list
		list->first = list->first->next;

		if(list->first) {
			// first element has no previous element
			list->first->prev = NULL;
		} else {
			// no first element => no last element either
			list->last = NULL;
		}
	}

	task->next = task->prev = NULL;
	return task;
}

#ifdef __cplusplus
}
#endif

#endif
