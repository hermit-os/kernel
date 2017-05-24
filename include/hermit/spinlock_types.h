/*
 * Copyright (c) 2011, Stefan Lankes, RWTH Aachen University
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
 * @file include/hermit/spinlock_types.h
 * @brief Spinlock type definition
 */

#ifndef __SPINLOCK_TYPES_H__
#define __SPINLOCK_TYPES_H__

#include <hermit/stddef.h>
#include <asm/atomic.h>

#ifdef __cplusplus
extern "C" {
#endif

/** @brief Spinlock structure */
typedef struct spinlock {
	/// Internal queue
	atomic_int64_t queue;
	/// Internal dequeue
	atomic_int64_t dequeue;
	/// Owner of this spinlock structure
	tid_t owner;
	/// Internal counter var
	uint32_t counter;
} spinlock_t;

typedef struct spinlock_irqsave {
	/// Internal queue
	atomic_int64_t queue;
	/// Internal dequeue
	atomic_int64_t dequeue;
	/// Core Id of the lock owner
	uint32_t coreid;
	/// Internal counter var
	uint32_t counter;
	/// Interrupt flag
	uint8_t flags;
} spinlock_irqsave_t;

/// Macro for spinlock initialization
#define SPINLOCK_INIT { ATOMIC_INIT(0), ATOMIC_INIT(1), MAX_TASKS, 0}
/// Macro for irqsave spinlock initialization
#define SPINLOCK_IRQSAVE_INIT { ATOMIC_INIT(0), ATOMIC_INIT(1), (uint32_t)-1, 0, 0}

#ifdef __cplusplus
}
#endif

#endif
