/*
 * Copyright (c) 2014, Steffen Vogel, RWTH Aachen University
 *                     All rights reserved.
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
 * @author Steffen Vogel <steffen.vogel@rwth-aachen.de>
 */

#ifndef __MALLOC_H__
#define __MALLOC_H__

#include <hermit/stddef.h>
#include <hermit/stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Binary exponent of maximal size for kmalloc()
#define BUDDY_MAX	32 // 4 GB
/// Binary exponent of minimal buddy size
#define BUDDY_MIN	6  // 64 Byte >= cache line
/// Binary exponent of the size which we allocate with buddy_fill()
#define BUDDY_ALLOC	16 // 64 KByte = 16 * PAGE_SIZE

#define BUDDY_LISTS	(BUDDY_MAX-BUDDY_MIN+1)
#define BUDDY_MAGIC	0xBABE

union buddy;

/** @brief Buddy
 *
 * Every free memory block is stored in a linked list according to its size.
 *  We can use this free memory to store this buddy_t union which represents
 *  this block (the buddy_t union is alligned to the front).
 *  Therefore the address of the buddy_t union is equal with the address
 *  of the underlying free memory block.
 *
 * Every allocated memory block is prefixed with its binary size exponent and
 *  a known magic number. This prefix is hidden by the user because its located
 *  before the actual memory address returned by kmalloc()
 */
typedef union buddy {
	/// Pointer to the next buddy in the linked list.
	union buddy* next;
	struct {
		/// The binary exponent of the block size
		uint8_t exponent;
		/// Must be equal to BUDDY_MAGIC for a valid memory block
		uint16_t magic;
	} prefix;
} buddy_t;

/** @brief Dump free buddies */
void buddy_dump(void);

#ifdef __cplusplus
}
#endif

#endif
