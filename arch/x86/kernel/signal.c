/*
 * Copyright (c) 2017, Daniel Krebs, RWTH Aachen University
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
#include <hermit/signal.h>
#include <hermit/stddef.h>
#include <hermit/spinlock.h>
#include <hermit/stdio.h>
#include <hermit/tasks.h>
#include <hermit/dequeue.h>
#include <hermit/logging.h>
#include <asm/apic.h>
#include <asm/irq.h>
#include <asm/atomic.h>

#define SIGNAL_IRQ (32 + 82)
#define SIGNAL_BUFFER_SIZE (16)

// Per-core signal queue and buffer
static dequeue_t signal_queue[MAX_CORES];
static sig_t signal_buffer[MAX_CORES][SIGNAL_BUFFER_SIZE];

static void _signal_irq_handler(struct state* s)
{
	LOG_DEBUG("Enter _signal_irq_handler() on core %d\n", CORE_ID);

	sig_t signal;
	task_t* dest_task;
	task_t* curr_task = per_core(current_task);

	while(dequeue_pop(&signal_queue[CORE_ID], &signal) == 0) {
		LOG_DEBUG("  Deliver signal %d\n", signal.signum);

		if(get_task(signal.dest, &dest_task) == 0) {
			LOG_DEBUG("  Found valid task with ID %d\n", dest_task->id);

			// only service signals for tasks on this core
			if(dest_task->last_core != CORE_ID) {
				LOG_DEBUG("  Signal dispatched to wrong CPU! Dropping it ...\n");
				continue;
			}

			if(dest_task->signal_handler) {
				LOG_DEBUG("  Has signal handler (%p)\n", dest_task->signal_handler);

				/* We will inject the signal handler into the control flow when
				 * the task will continue it's exection the next time. There are
				 * 3 cases how the task was interrupted:
				 *
				 *   1. call to reschedule() by own intend
				 *   2. a timer interrupt lead to rescheduling to another task
				 *   3. this IRQ interrupted the task
				 *
				 * Depending on those cases, the state of the task can either be
				 * saved to it's own stack (1.), it's interrupt stack (IST, 2.)
				 * or the stack of this interrupt handler (3.).
				 *
				 * When the signal handler finishes it's execution, we need to
				 * restore the task state, so we make the signal handler return
				 * first to sighandler_epilog() which then restores the original
				 * state.
				 *
				 * For cases 2+3, when task was interrupted by an IRQ, we modify
				 * the existing state on the interrupt stack to execute the
				 * signal handler, wherease in case 1, we craft a new state and
				 * place it on top of the task stack.
				 *
				 * The task stack will have the following layout:
				 *
				 * |         ...          | <- task's rsp before interruption
				 * |----------------------|
				 * |     saved state      |
				 * |----------------------|
				 * | &sighandler_epilog() | <- rsp after IRQ
				 * |----------------------|
				 * |----------------------| Only for case 1:
				 * | signal handler state | Craft signal handler state, so it
				 * |----------------------| executes before task is continued
				 */

				size_t* task_stackptr;
				struct state *task_state, *sighandler_state;

				const int task_is_running = dest_task == curr_task;
				LOG_DEBUG("  Task is%s running\n", task_is_running ? "" : " not");

				// location of task state depends of type of interruption
				task_state = (!task_is_running) ?
				/* case 1+2: */	(struct state*) dest_task->last_stack_pointer :
				/* case 3:   */ s;

				// pseudo state pushed by reschedule() has INT no. 0
				const int state_on_task_stack = task_state->int_no == 0;

				if(state_on_task_stack) {
					LOG_DEBUG("  State is already on task stack\n");
					// stack pointer was saved by switch_context() after saving
					// task state to task stack
					task_stackptr = dest_task->last_stack_pointer;
				} else {
					// stack pointer is last rsp, since task state is saved to
					// interrupt stack
					task_stackptr = (size_t*) task_state->rsp;

					LOG_DEBUG("  Copy state to task stack\n");
					task_stackptr -= sizeof(struct state) / sizeof(size_t);
					memcpy(task_stackptr, task_state, sizeof(struct state));
				}

				// signal handler will return to this function to restore
				// register state
				extern void sighandler_epilog();
				*(--task_stackptr) = (uint64_t) &sighandler_epilog;
				size_t* sighandler_rsp = task_stackptr;

				if(state_on_task_stack) {
					LOG_DEBUG("  Craft state for signal handler on task stack\n");

					// we actually only care for ss, rflags, cs, fs and gs
					task_stackptr -= sizeof(struct state) / sizeof(size_t);
					sighandler_state = (struct state*) task_stackptr;
					memcpy(sighandler_state, task_state, sizeof(struct state));

					// advance stack pointer so signal handler state will be
					// restored first
					dest_task->last_stack_pointer = (size_t*) sighandler_state;
				} else {
					LOG_DEBUG("  Reuse state on IST for signal handler\n");
					sighandler_state = task_state;
				}

				// update rsp so that sighandler_epilog() will be executed
				// after signal handler
				sighandler_state->rsp = (uint64_t) sighandler_rsp;
				sighandler_state->userrsp = sighandler_state->rsp;

				// call signal handler instead of continuing task's execution
				sighandler_state->rdi = (uint64_t) signal.signum;
				sighandler_state->rip = (uint64_t) dest_task->signal_handler;
			} else {
				LOG_DEBUG("  No signal handler installed\n");
			}
		} else {
			LOG_DEBUG("  Task %d has already died\n", signal.dest);
		}
	}
	LOG_DEBUG("Leave _signal_irq_handler() on core %d\n", CORE_ID);
}

int hermit_signal(signal_handler_t handler)
{
	task_t* curr_task = per_core(current_task);
	curr_task->signal_handler = handler;

	return 0;
}

int hermit_kill(tid_t dest, int signum)
{
	task_t* task;
	if(BUILTIN_EXPECT(get_task(dest, &task), 0)) {
		LOG_ERROR("Trying to send signal %d to invalid task %d\n", signum, dest);
		return -ENOENT;
	}

	const tid_t dest_core = task->last_core;

	LOG_DEBUG("Send signal %d from task %d (core %d) to task %d (core %d)\n",
	        signum, per_core(current_task)->id, CORE_ID, dest, dest_core);

	if(task == per_core(current_task)) {
		LOG_DEBUG("  Deliver signal to itself, call handler immediately\n");

		if(task->signal_handler) {
			task->signal_handler(signum);
		}
		return 0;
	}

	sig_t signal = {dest, signum};
	if(dequeue_push(&signal_queue[dest_core], &signal)) {
		LOG_ERROR("  Cannot push signal to task's signal queue, dropping it\n");
		return -ENOMEM;
	}

	// send IPI to destination core
	LOG_DEBUG("  Send signal IPI (%d) to core %d\n", SIGNAL_IRQ, dest_core);
	apic_send_ipi(dest_core, SIGNAL_IRQ);

	return 0;
}

void signal_init()
{
	// initialize per-core signal queue
	for(int i = 0; i < MAX_CORES; i++) {
		dequeue_init(&signal_queue[i], signal_buffer[i],
		             SIGNAL_BUFFER_SIZE, sizeof(sig_t));
	}

	irq_install_handler(SIGNAL_IRQ, _signal_irq_handler);
}
