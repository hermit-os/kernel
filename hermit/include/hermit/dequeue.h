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
 * @file include/hermit/dequeue.h
 * @brief Double-ended queue implementation
 */

#include <hermit/stddef.h>
#include <hermit/spinlock.h>
#include <hermit/errno.h>
#include <string.h>

#ifndef __DEQUEUE_H__
#define __DEQUEUE_H__

#define NOT_NULL(dequeue) do {	\
	if(BUILTIN_EXPECT(!dequeue, 0)) {	\
	    return -EINVAL;					\
	}									\
	} while(0)

typedef struct _dequeue_t {
	size_t front;			///< point to first used entry
	size_t back;			///< point to first unused entry
	spinlock_t lock;		///< make dequeue thread safe
	char* buffer;			///< pointer to buffer that holds elements
	size_t buffer_length;	///< number of elements buffer can hold
	size_t element_size;	///< size of one element in buffer
} dequeue_t;

static inline int
dequeue_init(dequeue_t* dequeue, void* buffer, size_t buffer_length, size_t element_size)
{
	NOT_NULL(dequeue);

	dequeue->front = 0;
	dequeue->back = 0;

	dequeue->buffer = buffer;
	dequeue->buffer_length = buffer_length;
	dequeue->element_size = element_size;

	spinlock_init(&dequeue->lock);

	return 0;
}

static inline int
dequeue_push(dequeue_t* dequeue, void* v)
{
	NOT_NULL(dequeue);

	spinlock_lock(&dequeue->lock);

	size_t new_back = (dequeue->back + 1) % dequeue->buffer_length;
	if(new_back == dequeue->front) {
		spinlock_unlock(&dequeue->lock);
		return -EOVERFLOW;
	}

	memcpy(&dequeue->buffer[dequeue->back * dequeue->element_size],
	        v,
	        dequeue->element_size);

	dequeue->back = new_back;

	spinlock_unlock(&dequeue->lock);
	return 0;
}

static inline int
dequeue_pop(dequeue_t* dequeue, void* out)
{

	NOT_NULL(dequeue);
	NOT_NULL(out);

	spinlock_lock(&dequeue->lock);

	if(dequeue->front == dequeue->back) {
		spinlock_unlock(&dequeue->lock);
		return -ENOENT;
	}

	memcpy(out,
	       &dequeue->buffer[dequeue->front * dequeue->element_size],
	        dequeue->element_size);

	dequeue->front = (dequeue->front + 1) % dequeue->buffer_length;

	spinlock_unlock(&dequeue->lock);

	return 0;
}

#endif // __DEQUEUE_H__
