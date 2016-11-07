/*
 * Copyright (c) 2016, Stefan Lankes, RWTH Aachen University
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

#include <hermit/stddef.h>
#include <hermit/stdlib.h>
#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/spinlock.h>
#include <hermit/memory.h>
#include <hermit/logging.h>

#include <asm/atomic.h>
#include <asm/page.h>

typedef struct free_list {
	size_t start, end;
	struct free_list* next;
	struct free_list* prev;
} free_list_t;

extern size_t hbmem_base;
extern size_t hbmem_size;

static spinlock_t list_lock = SPINLOCK_INIT;

static free_list_t init_list = {0, 0, NULL, NULL};
static free_list_t* free_start = &init_list;

extern atomic_int64_t total_pages;
extern atomic_int64_t total_allocated_pages;
extern atomic_int64_t total_available_pages;

size_t hbmem_get_pages(size_t npages)
{
	size_t i, ret = 0;
	free_list_t* curr = free_start;

	if (BUILTIN_EXPECT(!npages, 0))
		return 0;
	if (BUILTIN_EXPECT(npages > atomic_int64_read(&total_available_pages), 0))
		return 0;

	spinlock_lock(&list_lock);

	while(curr) {
		i = (curr->end - curr->start) / PAGE_SIZE;
		if (i > npages) {
			ret = curr->start;
			curr->start += npages * PAGE_SIZE;
			goto out;
		} else if (i == npages) {
			ret = curr->start;
			if (curr->prev)
				curr->prev = curr->next;
			else
				free_start = curr->next;
			if (curr != &init_list)
				kfree(curr);
			goto out;
		}

		curr = curr->next;
	}
out:
	LOG_DEBUG("get_pages: ret 0%llx, curr->start 0x%llx, curr->end 0x%llx\n", ret, curr->start, curr->end);

	spinlock_unlock(&list_lock);

	if (ret) {
		atomic_int64_add(&total_allocated_pages, npages);
		atomic_int64_sub(&total_available_pages, npages);
	}

	return ret;
}

/* TODO: reunion of elements is still missing */
int hbmem_put_pages(size_t phyaddr, size_t npages)
{
	free_list_t* curr = free_start;

	if (BUILTIN_EXPECT(!phyaddr, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(!npages, 0))
		return -EINVAL;

	spinlock_lock(&list_lock);

	while(curr) {
		if (phyaddr+npages*PAGE_SIZE == curr->start) {
			curr->start = phyaddr;
			goto out;
		} else if (phyaddr == curr->end) {
			curr->end += npages*PAGE_SIZE;
			goto out;
		} if (phyaddr > curr->end) {
			free_list_t* n = kmalloc(sizeof(free_list_t));

			if (BUILTIN_EXPECT(!n, 0))
				goto out_err;

			/* add new element */
			n->start = phyaddr;
			n->end = phyaddr + npages * PAGE_SIZE;
			n->prev = curr;
			n->next = curr->next;
			curr->next = n;
		}

		curr = curr->next;
	}
out:
	spinlock_unlock(&list_lock);

	atomic_int64_sub(&total_allocated_pages, npages);
	atomic_int64_add(&total_available_pages, npages);

	return 0;

out_err:
	spinlock_unlock(&list_lock);

	return -ENOMEM;
}

int is_hbmem_available(void)
{
	return (hbmem_base != 0);
}

int hbmemory_init(void)
{
	if (!hbmem_base)
		return 0;

	// determine available memory
	atomic_int64_add(&total_pages, hbmem_size >> PAGE_BITS);
	atomic_int64_add(&total_available_pages, hbmem_size >> PAGE_BITS);

	//initialize free list
	init_list.start = hbmem_base;
	init_list.end = hbmem_base + hbmem_size;

	LOG_INFO("free list for hbmem starts at 0x%zx, limit 0x%zx\n", init_list.start, init_list.end);

	return 0;
}
