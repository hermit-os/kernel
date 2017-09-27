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
 * @file include/hermit/stdlib.h
 * @brief Kernel space malloc and free functions and conversion functions
 *
 * This file contains some memory alloc and free calls for the kernel
 * and conversion functions.
 */

#ifndef __STDLIB_H__
#define __STDLIB_H__

#include <hermit/config.h>
#include <hermit/stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

void NORETURN do_abort(void);

/** @brief General page allocator function
 *
 * This function allocates and maps whole pages.
 * Mapped memory will be tracked by the VMA subsystem.
 * The sz argument is rounded down to multiples of the page size.
 *
 * For allocations which are smaller than a page you should use
 * the buddy system allocator (kmalloc and kfree) to avoid fragmentation.
 *
 * @param sz Desired size of the new memory
 * @param flags Flags to for map_region(), vma_add()
 *
 * @return Pointer to the new memory range (page-aligned).
 */
void* palloc(size_t sz, uint32_t flags);

/** @brief The memory allocator function
 *
 * This allocator uses a buddy system to allocate memory.
 * Attention: memory is not aligned!
 *
 * @return Pointer to the new memory range
 */
void* kmalloc(size_t sz);

/** @brief Create a stack with guard pages
 */
void* create_stack(size_t sz);

/** @brief Destroy stack with its guard pages
 */
int destroy_stack(void* addr, size_t sz);

/** @brief Release memory back to the buddy system
 *
 * Every block of memory allocated by kmalloc() is prefixed with a buddy_t
 * which includes the the size of the allocated block.
 * This prefix is also used to re-insert the block into the linked list
 * of free buddies.
 *
 * Released memory will still be managed by the buddy system.
 * Pages are not unmapped.
 *
 * Note: adjacent buddies are currently not merged!
 *
 * @see buddy_t
 * @param addr The address to the memory block allocated by kmalloc()
 */
void kfree(void* addr);

/** @brief Kernel's more general memory allocator function.
 *
 * This function lets you choose flags for the newly allocated memory.
 * The new region is always page aligned.
 *
 * @param sz Desired size of the new memory
 * @param flags Flags to specify
 *
 * @return Pointer to the new memory range
 */
void* page_alloc(size_t sz, uint32_t flags);

/** @brief Kernel's more general release function.
 *
 * This function is the complement of page_allocation.
 *
 * @param addr Pointer to the memory range
 * @param sz Desired size of the new memory
 */
void page_free(void* addr, size_t sz);

/** @brief String to long
 *
 * @return Long value of the parsed numerical string
 */
long _strtol(const char* str, char** endptr, int base);

#define strtol(str, endptr, base)	_strtol((str), (endptr), (base))

/** @brief String to unsigned long
 *
 * @return Unsigned long value of the parsed numerical string
 */
unsigned long _strtoul(const char* nptr, char** endptr, int base);

#define strtoul(nptr, endptr, base)	_strtoul((nptr), (endptr), (base))

/** @brief ASCII to integer
 *
 * Convenience function using strtol().
 *
 * @return Integer value of the parsed numerical string
 */
static inline int atoi(const char *str)
{
	return (int)_strtol(str, (char **)NULL, 10);
}

/** @brief Checks whether c is a hexdecimal digit character.
 *
 * @return A value different from zero if indeed c is a hexadecimal digit.
 * Zero otherwise.
 */
static inline int isxdigit(int c)
{
	if ((c >= '0') && (c <= '9'))
		return 1;
	else if ((c >= 'A') && (c <= 'F'))
		return 1;
	else if ((c >= 'a') && (c <= 'f'))
		return 1;

	return 0;
}

#ifdef __cplusplus
}
#endif

#endif
