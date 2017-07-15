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
#include <hermit/memory.h>
#include <hermit/logging.h>

#include <asm/atomic.h>
#include <asm/page.h>
#include <asm/multiboot.h>

#define GAP_BELOW	0x100000ULL

extern uint64_t base;
extern uint64_t limit;

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

static spinlock_t list_lock = SPINLOCK_INIT;

static free_list_t init_list = {0, 0, NULL, NULL};
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
	LOG_DEBUG("get_pages: ret 0%llx, curr->start 0x%llx, curr->end 0x%llx\n", ret, curr->start, curr->end);

	spinlock_unlock(&list_lock);

	if (ret) {
		atomic_int64_add(&total_allocated_pages, npages);
		atomic_int64_sub(&total_available_pages, npages);
	}

	return ret;
}

DEFINE_PER_CORE(size_t, ztmp_addr, 0);

size_t get_zeroed_page(void)
{
	size_t phyaddr = get_page();
	size_t viraddr;
	uint8_t flags;

	if (BUILTIN_EXPECT(!phyaddr, 0))
		return 0;

	flags = irq_nested_disable();

	viraddr = per_core(ztmp_addr);
	if (BUILTIN_EXPECT(!viraddr, 0))
	{
		viraddr = vma_alloc(PAGE_SIZE, VMA_READ|VMA_WRITE|VMA_CACHEABLE);
		if (BUILTIN_EXPECT(!viraddr, 0))
			goto novaddr;

		LOG_DEBUG("Core %d uses 0x%zx as temporary address\n", CORE_ID, viraddr);
		set_per_core(ztmp_addr, viraddr);
	}

	__page_map(viraddr, phyaddr, 1, PG_GLOBAL|PG_RW|PG_PRESENT, 0);

	memset((void*) viraddr, 0x00, PAGE_SIZE);

novaddr:
	irq_nested_enable(flags);

	return phyaddr;
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

void* page_alloc(size_t sz, uint32_t flags)
{
	size_t viraddr = 0;
	size_t phyaddr;
	uint32_t npages = PAGE_CEIL(sz) >> PAGE_BITS;
	size_t pflags = PG_PRESENT|PG_GLOBAL|PG_XD;

	if (BUILTIN_EXPECT(!npages, 0))
		goto oom;

	viraddr = vma_alloc(PAGE_CEIL(sz), flags);
	if (BUILTIN_EXPECT(!viraddr, 0))
		goto oom;

	phyaddr = get_pages(npages);
	if (BUILTIN_EXPECT(!phyaddr, 0))
	{
		vma_free(viraddr, viraddr+npages*PAGE_SIZE);
		viraddr = 0;
		goto oom;
	}

	if (flags & VMA_WRITE)
		pflags |= PG_RW;
	if (!(flags & VMA_CACHEABLE))
		pflags |= PG_PCD;

	int ret = page_map(viraddr, phyaddr, npages, pflags);
	if (BUILTIN_EXPECT(ret, 0))
	{
		vma_free(viraddr, viraddr+npages*PAGE_SIZE);
		put_pages(phyaddr, npages);
		viraddr = 0;
	}

oom:
	return (void*) viraddr;
}

void page_free(void* viraddr, size_t sz)
{
	size_t phyaddr;

	if (BUILTIN_EXPECT(!viraddr || !sz, 0))
		return;

	phyaddr = virt_to_phys((size_t)viraddr);

	vma_free((size_t) viraddr, (size_t) viraddr + PAGE_CEIL(sz));

	if (phyaddr)
		put_pages(phyaddr, PAGE_CEIL(sz) >> PAGE_BITS);
}

int memory_init(void)
{
	int ret = 0;

	// enable paging and map Multiboot modules etc.
	ret = page_init();
	if (BUILTIN_EXPECT(ret, 0)) {
		LOG_ERROR("Failed to initialize paging!\n");
		return ret;
	}

	LOG_INFO("mb_info: 0x%zx\n", mb_info);
	LOG_INFO("memory_init: base 0x%zx, image_size 0x%zx, limit 0x%zx\n", base, image_size, limit);

	if (mb_info) {
		if (mb_info->flags & MULTIBOOT_INFO_MEM_MAP) {
			size_t end_addr, start_addr;
			multiboot_memory_map_t* mmap = (multiboot_memory_map_t*) ((size_t) mb_info->mmap_addr);
			multiboot_memory_map_t* mmap_end = (void*) ((size_t) mb_info->mmap_addr + mb_info->mmap_length);

			// mark first available memory slot as free
			for(; mmap < mmap_end; mmap = (multiboot_memory_map_t*) ((size_t) mmap + sizeof(uint32_t) + mmap->size)) {
				if (mmap->type == MULTIBOOT_MEMORY_AVAILABLE) {
					start_addr = PAGE_CEIL(mmap->addr);
					end_addr = PAGE_FLOOR(mmap->addr + mmap->len);

					LOG_INFO("Free region 0x%zx - 0x%zx\n", start_addr, end_addr);

					if ((start_addr <= base) && (end_addr >= PAGE_2M_FLOOR((size_t) &kernel_start + image_size))) {
						init_list.start = PAGE_2M_CEIL((size_t) &kernel_start + image_size);
						init_list.end = end_addr;

						LOG_INFO("Add region 0x%zx - 0x%zx\n", init_list.start, init_list.end);
					}

					// determine available memory
					atomic_int64_add(&total_pages, (end_addr-start_addr) >> PAGE_BITS);
					atomic_int64_add(&total_available_pages, (end_addr-start_addr) >> PAGE_BITS);
				}
			}

			if (!init_list.end)
				goto oom;
		} else {
			goto oom;
		}
	} else {
		// determine available memory
		atomic_int64_add(&total_pages, (limit-base) >> PAGE_BITS);
		atomic_int64_add(&total_available_pages, (limit-base) >> PAGE_BITS);

		init_list.start = PAGE_2M_CEIL(base + image_size);
		init_list.end = limit;
	}

	// determine allocated memory, we use 2MB pages to map the kernel
	atomic_int64_add(&total_allocated_pages, PAGE_2M_CEIL(image_size) >> PAGE_BITS);
	atomic_int64_sub(&total_available_pages, PAGE_2M_CEIL(image_size) >> PAGE_BITS);

	LOG_INFO("free list starts at 0x%zx, limit 0x%zx\n", init_list.start, init_list.end);

	// init high bandwidth memory subsystem
	hbmemory_init();

	ret = vma_init();
	if (BUILTIN_EXPECT(ret, 0))
		LOG_WARNING("Failed to initialize VMA regions: %d\n", ret);

	// add missing free regions
	if (mb_info) {
		if (mb_info->flags & MULTIBOOT_INFO_MEM_MAP) {
			free_list_t* last = &init_list;
			size_t end_addr, start_addr;
			multiboot_memory_map_t* mmap = (multiboot_memory_map_t*) ((size_t) mb_info->mmap_addr);
			multiboot_memory_map_t* mmap_end = (void*) ((size_t) mb_info->mmap_addr + mb_info->mmap_length);

			// mark available memory as free
			for(; mmap < mmap_end; mmap = (multiboot_memory_map_t*) ((size_t) mmap + sizeof(uint32_t) + mmap->size))
			{
				if (mmap->type == MULTIBOOT_MEMORY_AVAILABLE) {
					start_addr = PAGE_CEIL(mmap->addr);
					end_addr = PAGE_FLOOR(mmap->addr + mmap->len);

					if ((start_addr <= base) && (end_addr >= PAGE_2M_CEIL(base+image_size)))
						end_addr = base;

					// ignore everything below 1M => reserve for I/O devices
					if ((start_addr < GAP_BELOW))
						start_addr = GAP_BELOW;

					if (start_addr < (size_t)mb_info)
						start_addr = PAGE_CEIL((size_t)mb_info);

					if ((mb_info->flags & MULTIBOOT_INFO_CMDLINE) && cmdline) {
						if (start_addr < (size_t) cmdline+cmdsize)
							start_addr = PAGE_CEIL((size_t) cmdline+cmdsize);
					}

					if (start_addr >= end_addr)
						continue;

					last->next = kmalloc(sizeof(free_list_t));
					if (BUILTIN_EXPECT(!last->next, 0))
						goto oom;

					LOG_INFO("Add region 0x%zx - 0x%zx\n", start_addr, end_addr);

					last->next->prev = last;
					last = last->next;
					last->next = NULL;
					last->start = start_addr;
					last->end = end_addr;
				}
			}
		}
	}

	return ret;

oom:
	LOG_ERROR("BUG: Failed to init mm!\n");
	while(1) {HALT; }
}
