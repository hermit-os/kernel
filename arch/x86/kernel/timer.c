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

#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/processor.h>
#include <hermit/time.h>
#include <hermit/tasks.h>
#include <hermit/errno.h>
#include <hermit/spinlock.h>
#include <hermit/logging.h>
#include <asm/irq.h>
#include <asm/irqflags.h>
#include <asm/io.h>

/*
 * This will keep track of how many ticks the system
 * has been running for
 */
DEFINE_PER_CORE(uint64_t, timer_ticks, 0);
extern uint32_t cpu_freq;
extern int32_t boot_processor;

#ifdef DYNAMIC_TICKS
DEFINE_PER_CORE(uint64_t, last_rdtsc, 0);
uint64_t boot_tsc = 0;

void check_ticks(void)
{
	// do we already know the cpu frequency? => if not, ignore this check
	if (!cpu_freq)
		return;

	const uint64_t curr_rdtsc = has_rdtscp() ? rdtscp(NULL) : rdtsc();
	rmb();

	const uint64_t diff_cycles = curr_rdtsc - per_core(last_rdtsc);
	const uint64_t cpu_freq_hz = 1000000ULL * (uint64_t) get_cpu_frequency();
	const uint64_t diff_ticks = (diff_cycles * (uint64_t) TIMER_FREQ) / cpu_freq_hz;

	if (diff_ticks > 0) {
		set_per_core(timer_ticks, per_core(timer_ticks) + diff_ticks);
		set_per_core(last_rdtsc, curr_rdtsc);
		rmb();
	}
}
#endif

/*
 * Handles the timer. In this case, it's very simple: We
 * increment the 'timer_ticks' variable every time the
 * timer fires.
 */
static void timer_handler(struct state *s)
{
#ifndef DYNAMIC_TICKS
	/* Increment our 'tick counter' */
	set_per_core(timer_ticks, per_core(timer_ticks)+1);
#else
	// do we already know the cpu frequency? => if not, use timer interrupt to count the ticks
	if (!cpu_freq)
		set_per_core(timer_ticks, per_core(timer_ticks)+1);
#endif

#if 0
	/*
	 * Every TIMER_FREQ clocks (approximately 1 second), we will
	 * display a message on the screen
	 */
	if (timer_ticks % TIMER_FREQ == 0) {
		LOG_INFO("One second has passed %d\n", CORE_ID);
	}
#endif
}

int timer_wait(unsigned int ticks)
{
	uint64_t eticks = per_core(timer_ticks) + ticks;

	task_t* curr_task = per_core(current_task);

	if (curr_task->status == TASK_IDLE)
	{
		/*
		 * This will continuously loop until the given time has
		 * been reached
		 */
		while (per_core(timer_ticks) < eticks) {
			check_workqueues();

			// recheck break condition
			if (per_core(timer_ticks) >= eticks)
				break;

			PAUSE;
		}
	} else if (per_core(timer_ticks) < eticks) {
		check_workqueues();

		if (per_core(timer_ticks) < eticks) {
			set_timer(eticks);
			reschedule();
		}
	}

	return 0;
}

#define LATCH(f)	((CLOCK_TICK_RATE + f/2) / f)
#define WAIT_SOME_TIME() do { uint64_t start = rdtsc(); mb(); \
			      while(rdtsc() - start < 1000000) ; \
			} while (0)

static int pit_init(void)
{
	/*
	 * Port 0x43 is for initializing the PIT:
	 *
	 * 0x34 means the following:
	 * 0b...     (step-by-step binary representation)
	 * ...  00  - channel 0
	 * ...  11  - write two values to counter register:
	 *            first low-, then high-byte
	 * ... 010  - mode number 2: "rate generator" / frequency divider
	 * ...   0  - binary counter (the alternative is BCD)
	 */
	outportb(0x43, 0x34);

	WAIT_SOME_TIME();

	/* Port 0x40 is for the counter register of channel 0 */

	outportb(0x40, LATCH(TIMER_FREQ) & 0xFF);   /* low byte  */

	WAIT_SOME_TIME();

	outportb(0x40, LATCH(TIMER_FREQ) >> 8);     /* high byte */

	return 0;
}

/*
 * Sets up the system clock by installing the timer handler
 * into IRQ0
 */
int timer_init(void)
{
#ifdef DYNAMIC_TICKS
	if (boot_tsc)
	{
		set_per_core(last_rdtsc, boot_tsc);
		return 0;
	}
#endif

	/*
	 * Installs 'timer_handler' for the PIC and APIC timer,
	 * only one handler will be later used.
	 */
	irq_install_handler(32, timer_handler);
	irq_install_handler(123, timer_handler);

#ifdef DYNAMIC_TICKS
	boot_tsc = has_rdtscp() ? rdtscp(NULL) : rdtsc();
	set_per_core(last_rdtsc, boot_tsc);
#endif

	if (cpu_freq) // do we need to configure the timer?
		return 0;

	return pit_init();
}
