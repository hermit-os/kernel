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

#include <hermit/stdio.h>
#include <hermit/malloc.h>
#include <hermit/spinlock.h>
#include <hermit/memory.h>
#include <asm/page.h>

/// A linked list for each binary size exponent
static buddy_t* buddy_lists[BUDDY_LISTS] = { [0 ... BUDDY_LISTS-1] = NULL };
/// Lock for the buddy lists
static spinlock_t buddy_lock = SPINLOCK_INIT;

/** @brief Check if larger free buddies are available */
static inline int buddy_large_avail(uint8_t exp)
{
	while ((exp<BUDDY_MAX) && !buddy_lists[exp-BUDDY_MIN])
		exp++;

	return exp != BUDDY_MAX;
}

/** @brief Calculate the required buddy size */
static inline int buddy_exp(size_t sz)
{
	int exp;
	for (exp=0; sz>(1<<exp); exp++);

	if (exp > BUDDY_MAX)
		exp = 0;
	if (exp < BUDDY_MIN)
		exp = BUDDY_MIN;

	return exp;
}

/** @brief Get a free buddy by potentially splitting a larger one */
static buddy_t* buddy_get(int exp)
{
	spinlock_lock(&buddy_lock);
	buddy_t** list = &buddy_lists[exp-BUDDY_MIN];
	buddy_t* buddy = *list;
	buddy_t* split;

	if (buddy)
		// there is already a free buddy =>
		// we remove it from the list
		*list = buddy->next;
	else if ((exp >= BUDDY_ALLOC) && !buddy_large_avail(exp))
		// theres no free buddy larger than exp =>
		// we can allocate new memory
		buddy = (buddy_t*) palloc(1<<exp, 0);
	else {
		// we recursivly request a larger buddy...
		buddy = buddy_get(exp+1);
		if (BUILTIN_EXPECT(!buddy, 0))
			goto out;

		// ... and split it, by putting the second half back to the list
		split = (buddy_t*) ((size_t) buddy + (1<<exp));
		split->next = *list;
		*list = split;
	}

out:
	spinlock_unlock(&buddy_lock);

	return buddy;
}

/** @brief Put a buddy back to its free list
 *
 * TODO: merge adjacent buddies (memory compaction)
 */
static void buddy_put(buddy_t* buddy)
{
	spinlock_lock(&buddy_lock);
	buddy_t** list = &buddy_lists[buddy->prefix.exponent-BUDDY_MIN];
	buddy->next = *list;
	*list = buddy;
	spinlock_unlock(&buddy_lock);
}

void buddy_dump(void)
{
	size_t free = 0;
	int i;

	for (i=0; i<BUDDY_LISTS; i++) {
		buddy_t* buddy;
		int exp = i+BUDDY_MIN;

		if (buddy_lists[i])
			kprintf("buddy_list[%u] (exp=%u, size=%lu bytes):\n", i, exp, 1<<exp);

		for (buddy=buddy_lists[i]; buddy; buddy=buddy->next) {
			kprintf("  %p -> %p \n", buddy, buddy->next);
			free += 1<<exp;
		}
	}
	kprintf("free buddies: %lu bytes\n", free);
}

void* palloc(size_t sz, uint32_t flags)
{
	size_t phyaddr, viraddr;
	uint32_t npages = PAGE_FLOOR(sz) >> PAGE_BITS;
	int err;

	//kprintf("palloc(%lu) (%lu pages)\n", sz, npages);

	// get free virtual address space
	viraddr = vma_alloc(npages*PAGE_SIZE, VMA_HEAP);
	if (BUILTIN_EXPECT(!viraddr, 0))
		return NULL;

	// get continous physical pages
	phyaddr = get_pages(npages);
	if (BUILTIN_EXPECT(!phyaddr, 0)) {
		vma_free(viraddr, viraddr+npages*PAGE_SIZE);
		return NULL;
	}

	// map physical pages to VMA
	err = page_map(viraddr, phyaddr, npages, PG_RW|PG_GLOBAL);
	if (BUILTIN_EXPECT(err, 0)) {
		vma_free(viraddr, viraddr+npages*PAGE_SIZE);
		put_pages(phyaddr, npages);
		return NULL;
	}

	return (void*) viraddr;
}

void pfree(void* addr, size_t sz)
{
	if (BUILTIN_EXPECT(!addr || !sz, 0))
		return;

	size_t i;
	size_t phyaddr;
	size_t viraddr = (size_t) addr & PAGE_MASK;
	uint32_t npages = PAGE_FLOOR(sz) >> PAGE_BITS;

	// memory is probably not continuously mapped! (userspace heap)
	for (i=0; i<npages; i++) {
		phyaddr = virt_to_phys(viraddr+i*PAGE_SIZE);
		put_page(phyaddr);
	}

	page_unmap(viraddr, npages);
	vma_free(viraddr, viraddr+npages*PAGE_SIZE);
}

void* kmalloc(size_t sz)
{
	if (BUILTIN_EXPECT(!sz, 0))
		return NULL;

	// add space for the prefix
	sz += sizeof(buddy_t);

	int exp = buddy_exp(sz);
	if (BUILTIN_EXPECT(!exp, 0))
		return NULL;

	buddy_t* buddy = buddy_get(exp);
	if (BUILTIN_EXPECT(!buddy, 0))
		return NULL;

	// setup buddy prefix
	buddy->prefix.magic = BUDDY_MAGIC;
	buddy->prefix.exponent = exp;

	//kprintf("kmalloc(%lu) = %p\n", sz, buddy+1);

	// pointer arithmetic: we hide the prefix
	return buddy+1;
}

void kfree(void *addr)
{
	if (BUILTIN_EXPECT(!addr, 0))
		return;

	//kprintf("kfree(%lu)\n", addr);

	buddy_t* buddy = (buddy_t*) addr - 1; // get prefix

	// check magic
	if (BUILTIN_EXPECT(buddy->prefix.magic != BUDDY_MAGIC, 0))
		return;

	buddy_put(buddy);
}
