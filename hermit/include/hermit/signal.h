/*
 * Copyright (c) 2016, Daniel Krebs, RWTH Aachen University
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
 * @author Daniel Krebs
 * @file include/hermit/signal.h
 * @brief Signal related functions
 */

#ifndef __SIGNAL_H__
#define __SIGNAL_H__

#ifdef __cplusplus
extern "C" {
#endif

#include <hermit/stddef.h>
#include <hermit/semaphore_types.h>

#define MAX_SIGNALS 32

typedef void (*signal_handler_t)(int);

// This is used in deqeue.h (HACK)
typedef struct _sig {
	tid_t dest;
	int signum;
} sig_t;

/** @brief Send signal to kernel task
 *
 * @param dest		Send signal to this task
 * @param signum	Signal number
 * @return
 *  - 0 on success
 *  - -ENOENT (-2) if task not found
 */
int hermit_kill(tid_t dest, int signum);

/** @brief Register signal handler
 *
 * @param handler	Signal handler
 * @return
 *  - 0 on success
 */
int hermit_signal(signal_handler_t handler);

#ifdef __cplusplus
}
#endif

#endif // __SIGNAL_H__
