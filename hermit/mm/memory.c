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

#include <hermit/stddef.h>
#include <hermit/stdlib.h>
#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/spinlock.h>

#include <asm/atomic.h>
#include <asm/page.h>

extern uint64_t base;
extern uint64_t limit;
extern uint64_t image_size;

typedef struct free_list {
	size_t start, end;
	struct free_list* next;
	struct free_list* prev;
} free_list_t;

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void kernel_start;
extern const void kernel_end;

static spinlock_t list_lock = SPINLOCK_INIT;

static free_list_t init_list;
static free_list_t* free_start = &init_list;

atomic_int64_t total_pages = ATOMIC_INIT(0);
atomic_int64_t total_allocated_pages = ATOMIC_INIT(0);
atomic_int64_t total_available_pages = ATOMIC_INIT(0);

size_t get_pages(size_t npages)
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
	//kprintf("get_pages: ret 0%llx, curr->start 0x%llx, curr->end 0x%llx\n", ret, curr->start, curr->end);

	spinlock_unlock(&list_lock);

	if (ret) {
		atomic_int64_add(&total_allocated_pages, npages);
		atomic_int64_sub(&total_available_pages, npages);
	}

	return ret;
}

/* TODO: reunion of elements is still missing */
int put_pages(size_t phyaddr, size_t npages)
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

int copy_page(size_t pdest, size_t psrc)
{
	int err;

	static size_t viraddr;
	if (!viraddr) { // statically allocate virtual memory area
		viraddr = vma_alloc(2 * PAGE_SIZE, VMA_HEAP);
		if (BUILTIN_EXPECT(!viraddr, 0))
			return -ENOMEM;
	}

	// map pages
	size_t vsrc = viraddr;
	err = page_map(vsrc, psrc, 1, PG_GLOBAL|PG_RW);
	if (BUILTIN_EXPECT(err, 0)) {
		page_unmap(viraddr, 1);
		return -ENOMEM;
	}

	size_t vdest = viraddr + PAGE_SIZE;
	err = page_map(vdest, pdest, 1, PG_GLOBAL|PG_RW);
	if (BUILTIN_EXPECT(err, 0)) {
		page_unmap(viraddr + PAGE_SIZE, 1);
		return -ENOMEM;
	}

	kprintf("copy_page: copy page frame from: %#lx (%#lx) to %#lx (%#lx)\n", vsrc, psrc, vdest, pdest); // TODO remove

	// copy the whole page
	memcpy((void*) vdest, (void*) vsrc, PAGE_SIZE);

	// householding
	page_unmap(viraddr, 2);

	return 0;
}

int memory_init(void)
{
	size_t addr;
	int ret = 0;

	// enable paging and map Multiboot modules etc.
	ret = page_init();
	if (BUILTIN_EXPECT(ret, 0)) {
		kputs("Failed to initialize paging!\n");
		return ret;
	}

	kprintf("memory_init: base 0x%zx, image_size 0x%zx, limit 0x%zx\n", base, image_size, limit);

	// mark available memory as free
	for(addr=base; addr<limit; addr+=PAGE_SIZE) {
		atomic_int64_inc(&total_pages);
		atomic_int64_inc(&total_available_pages);
	}

	// mark kernel as used, we use 2MB pages to map the kernel
	for(addr=base; addr<((base + image_size + 0x200000) & 0xFFFFFFFFFFE00000ULL); addr+=PAGE_SIZE) {
		atomic_int64_inc(&total_allocated_pages);
		atomic_int64_dec(&total_available_pages);
	}

	//initialize free list
	init_list.start = (base + image_size + 0x200000) & 0xFFFFFFFFFFE00000ULL;
	init_list.end = limit;
	init_list.prev = init_list.next = NULL;

	ret = vma_init();
	if (BUILTIN_EXPECT(ret, 0))
		kprintf("Failed to initialize VMA regions: %d\n", ret);

	return ret;
}
