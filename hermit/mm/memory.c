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
#include <asm/multiboot.h>
#include <asm/page.h>

extern uint32_t base;
extern uint32_t limit;

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void kernel_start;
extern const void kernel_end;

static char stack[MAX_TASKS-1][KERNEL_STACK_SIZE];
static char bitmap[BITMAP_SIZE];

static spinlock_t bitmap_lock = SPINLOCK_INIT;

atomic_int32_t total_pages = ATOMIC_INIT(0);
atomic_int32_t total_allocated_pages = ATOMIC_INIT(0);
atomic_int32_t total_available_pages = ATOMIC_INIT(0);

void* create_stack(tid_t id)
{
	// idle task uses stack, which is defined in entry.asm
	if (BUILTIN_EXPECT(!id, 0))
		return NULL;
	// do we have a valid task id?
	if (BUILTIN_EXPECT(id >= MAX_TASKS, 0))
		return NULL;

	return (void*) stack[id-1];
}

inline static int page_marked(size_t i)
{
	size_t index = i >> 3;
	size_t mod = i & 0x7;

	return  (bitmap[index] & (1 << mod));
}

inline static void page_set_mark(size_t i)
{
	size_t index = i >> 3;
	size_t mod = i & 0x7;

	bitmap[index] = bitmap[index] | (1 << mod);
}

inline static void page_clear_mark(size_t i)
{
	size_t index = i / 8;
	size_t mod = i % 8;

	bitmap[index] = bitmap[index] & ~(1 << mod);
}

size_t get_pages(size_t npages)
{
	size_t cnt, off;
	static size_t alloc_start = (size_t) -1;

	if (BUILTIN_EXPECT(!npages, 0))
		return 0;
	if (BUILTIN_EXPECT(npages > atomic_int32_read(&total_available_pages), 0))
		return 0;

	spinlock_lock(&bitmap_lock);

	if (alloc_start == (size_t)-1)
		 alloc_start = ((size_t) &kernel_end >> PAGE_BITS);
	off = 1;
	while (off <= BITMAP_SIZE*8 - npages) {
		for (cnt=0; cnt<npages; cnt++) {
			if (page_marked(((off+alloc_start)%(BITMAP_SIZE*8 - npages))+cnt))
				goto next;
		}

		off = (off+alloc_start) % (BITMAP_SIZE*8 - npages);
		alloc_start = off+npages;

		for (cnt=0; cnt<npages; cnt++) {
			page_set_mark(off+cnt);
		}

		spinlock_unlock(&bitmap_lock);

		atomic_int32_add(&total_allocated_pages, npages);
		atomic_int32_sub(&total_available_pages, npages);

		return off << PAGE_BITS;

next:		off += cnt+1;
	}

	spinlock_unlock(&bitmap_lock);

	return 0;
}

int put_pages(size_t phyaddr, size_t npages)
{
	size_t i, ret = 0;
	size_t base = phyaddr >> PAGE_BITS;

	if (BUILTIN_EXPECT(!phyaddr, 0))
		return -EINVAL;
	if (BUILTIN_EXPECT(!npages, 0))
		return -EINVAL;

	spinlock_lock(&bitmap_lock);

	for (i=0; i<npages; i++) {
		if (page_marked(base+i)) {
			page_clear_mark(base+i);
			ret++;
		}
	}

	spinlock_unlock(&bitmap_lock);

	atomic_int32_sub(&total_allocated_pages, ret);
	atomic_int32_add(&total_available_pages, ret);

	return ret;
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
	unsigned int i;
	size_t addr;
	int ret = 0;

	// mark all memory as used
	memset(bitmap, 0xff, BITMAP_SIZE);

	// enable paging and map Multiboot modules etc.
	ret = page_init();
	if (BUILTIN_EXPECT(ret, 0)) {
		kputs("Failed to initialize paging!\n");
		return ret;
	}

	// parse multiboot information for available memory
	if (mb_info) {
		if (mb_info->flags & MULTIBOOT_INFO_MEM_MAP) {
			size_t end_addr;
			multiboot_memory_map_t* mmap = (multiboot_memory_map_t*) ((size_t) mb_info->mmap_addr);
			multiboot_memory_map_t* mmap_end = (void*) ((size_t) mb_info->mmap_addr + mb_info->mmap_length);

			// mark available memory as free
			while (mmap < mmap_end) {
				if (mmap->type == MULTIBOOT_MEMORY_AVAILABLE) {
					/* set the available memory as "unused" */
					addr = mmap->addr;
					end_addr = addr + mmap->len;

					while ((addr < end_addr) && (addr < (BITMAP_SIZE*8*PAGE_SIZE))) {
						if (page_marked(addr >> PAGE_BITS)) {
							page_clear_mark(addr >> PAGE_BITS);
							atomic_int32_inc(&total_pages);
							atomic_int32_inc(&total_available_pages);
						}
						addr += PAGE_SIZE;
					}
				}
				mmap = (multiboot_memory_map_t*) ((size_t) mmap + sizeof(uint32_t) + mmap->size);
			}
		} else if (mb_info->flags & MULTIBOOT_INFO_MEM) {
			size_t page;
			size_t pages_lower = mb_info->mem_lower >> 2; /* KiB to page number */
			size_t pages_upper = mb_info->mem_upper >> 2;

			for (page=0; page<pages_lower; page++)
				page_clear_mark(page);

			if (pages_upper > BITMAP_SIZE*8-256)
				pages_upper = BITMAP_SIZE*8-256;

			for (page=0; page<pages_upper; page++)
				page_clear_mark(page + 256); /* 1 MiB == 256 pages offset */

			atomic_int32_add(&total_pages, pages_lower + pages_upper);
			atomic_int32_add(&total_available_pages, pages_lower + pages_upper);
		} else {
			kputs("Unable to initialize the memory management subsystem\n");
			while (1) HALT;
		}

		// mark mb_info as used
		page_set_mark((size_t) mb_info >> PAGE_BITS);
		atomic_int32_inc(&total_allocated_pages);
		atomic_int32_dec(&total_available_pages);


		if (mb_info->flags & MULTIBOOT_INFO_MODS) {
			// mark modules list as used
			for(addr=mb_info->mods_addr; addr<mb_info->mods_addr+mb_info->mods_count*sizeof(multiboot_module_t); addr+=PAGE_SIZE) {
				page_set_mark(addr >> PAGE_BITS);
				atomic_int32_inc(&total_allocated_pages);
				atomic_int32_dec(&total_available_pages);
			}

			// mark modules as used
			multiboot_module_t* mmodule = (multiboot_module_t*) ((size_t) mb_info->mods_addr);
			for(i=0; i<mb_info->mods_count; i++) {
				for(addr=mmodule[i].mod_start; addr<mmodule[i].mod_end; addr+=PAGE_SIZE) {
					page_set_mark(addr >> PAGE_BITS);
					atomic_int32_inc(&total_allocated_pages);
					atomic_int32_dec(&total_available_pages);
				}
			}
		}

		// mark kernel as used, we use 2MB pages to map the kernel
		for(addr=(size_t) &kernel_start; addr<(((size_t) &kernel_end + 0x200000ULL) & 0xFFFFFFFFFFE00000ULL); addr+=PAGE_SIZE) {
			page_set_mark(addr >> PAGE_BITS);
			atomic_int32_inc(&total_allocated_pages);
			atomic_int32_dec(&total_available_pages);
		}

	} else {
		//kprintf("base 0x%lx, limit 0x%lx\n", base, limit);

		// mark available memory as free
		for(addr=base+0x200000ULL; (addr<limit) && (addr < (BITMAP_SIZE*8*PAGE_SIZE)); addr+=PAGE_SIZE) {
			if (page_marked(addr >> PAGE_BITS)) {
				page_clear_mark(addr >> PAGE_BITS);
				atomic_int32_inc(&total_pages);
				atomic_int32_inc(&total_available_pages);
			}
		}

		atomic_int32_add(&total_allocated_pages, 0x200000 / PAGE_SIZE);
		atomic_int32_add(&total_pages, 0x200000 / PAGE_SIZE);
	}

	ret = vma_init();
	if (BUILTIN_EXPECT(ret, 0)) {
		kprintf("Failed to initialize VMA regions: %d\n", ret);
		return ret;
	}

	/*
	 * Modules like the init ram disk are already loaded.
	 * Therefore, we set these pages as used.
	 */
	if (mb_info && (mb_info->flags & MULTIBOOT_INFO_MODS)) {
		multiboot_module_t* mmodule = (multiboot_module_t*) ((size_t) mb_info->mods_addr);
		for(i=0; i<mb_info->mods_count; i++) {
			for(addr=mmodule[i].mod_start; addr<mmodule[i].mod_end; addr+=PAGE_SIZE) {
				page_set_mark(addr >> PAGE_BITS);
				atomic_int32_inc(&total_allocated_pages);
				atomic_int32_dec(&total_available_pages);
			}
		}
	}

	return ret;
}
