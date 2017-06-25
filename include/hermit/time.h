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
 * @file include/hermit/time.h
 * @brief Time related functions
 */

#ifndef __TIME_H__
#define __TIME_H__

#include <asm/time.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint32_t clock_t;

struct tms {
	clock_t tms_utime;
	clock_t tms_stime;
	clock_t tms_cutime;
	clock_t tms_cstime;
};

/** @brief Initialize Timer interrupts
 *
 * This procedure installs IRQ handlers for timer interrupts
 */
int timer_init(void);

/** @brief Initialized a timer
 *
 * @param ticks Amount of ticks to wait
 * @return
 * - 0 on success
 */
int timer_wait(unsigned int ticks);

DECLARE_PER_CORE(uint64_t, timer_ticks);

/** @brief Returns the current number of ticks.
 * @return Current number of ticks
 */
static inline uint64_t get_clock_tick(void)
{
	return per_core(timer_ticks);
}

/** @brief sleep some seconds
 *
 * This function sleeps some seconds
 *
 * @param sec Amount of seconds to wait
 */
static inline void sleep(unsigned int sec) { timer_wait(sec*TIMER_FREQ); }

/** @brief Get milliseconds since system boot
 */
static inline uint64_t get_uptime() { return (get_clock_tick() * 1000) / TIMER_FREQ; }

#ifdef __cplusplus
}
#endif

#endif
