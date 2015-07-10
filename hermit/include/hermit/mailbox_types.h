/*
 * Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 *      modification, are permitted provided that the following conditions are met:
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
 * @file include/hermit/mailbox_types.h
 * @brief Message type structure definitions for various task return types
 */

#ifndef __MAILBOX_TYPES_H__
#define __MAILBOX_TYPES_H__

#include <hermit/semaphore_types.h>

#ifdef __cplusplus
extern "C" {
#endif

/** @brief Wait message structure
 *
 * This message struct keeps a recipient task id and the message itself */
typedef struct {
	/// The task id of the task which is waiting for this message
	tid_t	id;
	/// The message payload
	int32_t	result;
} wait_msg_t;

#define MAILBOX_TYPES(name, type) 	\
	typedef struct mailbox_##name { \
		type buffer[MAILBOX_SIZE]; \
		int wpos, rpos; \
		sem_t mails; \
		sem_t boxes; \
		spinlock_t rlock, wlock; \
	} mailbox_##name##_t;

MAILBOX_TYPES(wait_msg, wait_msg_t)
MAILBOX_TYPES(int32, int32_t)
MAILBOX_TYPES(int16, int16_t)
MAILBOX_TYPES(int8, int8_t)
MAILBOX_TYPES(uint32, uint32_t)
MAILBOX_TYPES(uint16, uint16_t)
MAILBOX_TYPES(uint8, uint8_t)
MAILBOX_TYPES(ptr, void*)

#ifdef __cplusplus
}
#endif

#endif
