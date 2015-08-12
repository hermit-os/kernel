/*
 * Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
 *               2014, Steffen Vogel, RWTH Aachen University
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
 * This is a 32/64 bit portable paging implementation for the x86 architecture
 * using self-referenced page tables	i.
 * See http://www.noteblok.net/2014/06/14/bachelor/ for a detailed description.
 * 
 * @author Steffen Vogel <steffen.vogel@rwth-aachen.de>
 */

#include <hermit/stdio.h>
#include <hermit/memory.h>
#include <hermit/errno.h>
#include <hermit/string.h>
#include <hermit/spinlock.h>
#include <hermit/tasks.h>

#include <asm/irq.h>
#include <asm/page.h>

/* Note that linker symbols are not variables, they have no memory
 * allocated for maintaining a value, rather their address is their value. */
extern const void kernel_start;
//extern const void kernel_end;

/// This page is reserved for copying
#define PAGE_TMP		(PAGE_FLOOR((size_t) &kernel_start) - PAGE_SIZE)

/** Lock for kernel space page tables */
static spinlock_t kslock = SPINLOCK_INIT;

/** This PGD table is initialized in entry.asm */
extern size_t* boot_map;

#if 0
/** A self-reference enables direct access to all page tables */
static size_t * const self[PAGE_LEVELS] = {
	(size_t *) 0xFFC00000,
	(size_t *) 0xFFFFF000
};

/** An other self-reference for page_map_copy() */
static size_t * const other[PAGE_LEVELS] = {
	(size_t *) 0xFF800000,
	(size_t *) 0xFFFFE000
};
#else
/** A self-reference enables direct access to all page tables */
static size_t* const self[PAGE_LEVELS] = {
	(size_t *) 0xFFFFFF8000000000,
	(size_t *) 0xFFFFFFFFC0000000,
	(size_t *) 0xFFFFFFFFFFE00000,
	(size_t *) 0xFFFFFFFFFFFFF000
};

/** An other self-reference for page_map_copy() */
static size_t * const other[PAGE_LEVELS] = {
	(size_t *) 0xFFFFFF0000000000,
	(size_t *) 0xFFFFFFFF80000000,
	(size_t *) 0xFFFFFFFFFFC00000,
	(size_t *) 0xFFFFFFFFFFFFE000
};
#endif

size_t virt_to_phys(size_t addr)
{
	size_t vpn   = addr >> PAGE_BITS;	// virtual page number
	size_t entry = self[0][vpn];		// page table entry
	size_t off   = addr  & ~PAGE_MASK;	// offset within page
	size_t phy   = entry &  PAGE_MASK;	// physical page frame number

	return phy | off;
}

//TODO: code is missing
int page_set_flags(size_t viraddr, uint32_t npages, int flags)
{
	return -EINVAL;
}

int page_map(size_t viraddr, size_t phyaddr, size_t npages, size_t bits)
{
	int lvl, ret = -ENOMEM;
	long vpn = viraddr >> PAGE_BITS;
	long first[PAGE_LEVELS], last[PAGE_LEVELS];
	task_t* curr_task;

	/* Calculate index boundaries for page map traversal */
	for (lvl=0; lvl<PAGE_LEVELS; lvl++) {
		first[lvl] = (vpn         ) >> (lvl * PAGE_MAP_BITS);
		last[lvl]  = (vpn+npages-1) >> (lvl * PAGE_MAP_BITS);
	}

	curr_task = per_core(current_task);

	/** @todo: might not be sufficient! */
	if (bits & PG_USER)
		spinlock_irqsave_lock(&curr_task->page_lock);
	else
		spinlock_lock(&kslock);

	/* Start iterating through the entries
	 * beginning at the root table (PGD or PML4) */
	for (lvl=PAGE_LEVELS-1; lvl>=0; lvl--) {
		for (vpn=first[lvl]; vpn<=last[lvl]; vpn++) {
			if (lvl) { /* PML4, PDPT, PGD */
				if (!(self[lvl][vpn] & PG_PRESENT)) {
					/* There's no table available which covers the region.
					 * Therefore we need to create a new empty table. */
					size_t phyaddr = get_pages(1);
					if (BUILTIN_EXPECT(!phyaddr, 0))
						goto out;
					
					if (bits & PG_USER)
						atomic_int64_inc(curr_task->user_usage);

					/* Reference the new table within its parent */
#if 0
					self[lvl][vpn] = phyaddr | bits | PG_PRESENT | PG_USER | PG_RW;
#else
					self[lvl][vpn] = (phyaddr | bits | PG_PRESENT | PG_USER | PG_RW) & ~PG_XD;
#endif

					/* Fill new table with zeros */
					memset(&self[lvl-1][vpn<<PAGE_MAP_BITS], 0, PAGE_SIZE);
				}
			}
			else { /* PGT */
				int8_t flush = 0;

				/* do we have to flush the TLB? */
				if (self[lvl][vpn] & PG_PRESENT)
					flush = 1;

				self[lvl][vpn] = phyaddr | bits | PG_PRESENT;

				if (flush)
					/* There's already a page mapped at this address.
					 * We have to flush a single TLB entry. */
					tlb_flush_one_page(vpn << PAGE_BITS);

				phyaddr += PAGE_SIZE;
			}
		}
	}

	ret = 0;
out:
	if (bits & PG_USER)
		spinlock_irqsave_unlock(&curr_task->page_lock);
	else
		spinlock_unlock(&kslock);

	return ret;
}

/** Tables are freed by page_map_drop() */
int page_unmap(size_t viraddr, size_t npages)
{
	task_t* curr_task = per_core(current_task);

	/* We aquire both locks for kernel and task tables
	 * as we dont know to which the region belongs. */
	spinlock_irqsave_lock(&curr_task->page_lock);
	spinlock_lock(&kslock);

	/* Start iterating through the entries.
	 * Only the PGT entries are removed. Tables remain allocated. */
	size_t vpn, start = viraddr>>PAGE_BITS;
	for (vpn=start; vpn<start+npages; vpn++)
		self[0][vpn] = 0;

	spinlock_irqsave_unlock(&curr_task->page_lock);
	spinlock_unlock(&kslock);

	/* This can't fail because we don't make checks here */
	return 0;
}

int page_map_drop(void)
{
	task_t* curr_task = per_core(current_task);

	void traverse(int lvl, long vpn) {
		long stop;
		for (stop=vpn+PAGE_MAP_ENTRIES; vpn<stop; vpn++) {
			if ((self[lvl][vpn] & PG_PRESENT) && (self[lvl][vpn] & PG_USER)) {
				/* Post-order traversal */
				if (lvl)
					traverse(lvl-1, vpn<<PAGE_MAP_BITS);

				put_pages(self[lvl][vpn] & PAGE_MASK, 1);
				atomic_int64_dec(curr_task->user_usage);
			}
		}
	}

	spinlock_irqsave_lock(&curr_task->page_lock);

	traverse(PAGE_LEVELS-1, 0);

	spinlock_irqsave_unlock(&curr_task->page_lock);

	/* This can't fail because we don't make checks here */
	return 0;
}

int page_map_copy(task_t *dest)
{
	task_t* curr_task = per_core(current_task);

	int traverse(int lvl, long vpn) {
		long stop;
		for (stop=vpn+PAGE_MAP_ENTRIES; vpn<stop; vpn++) {
			if (self[lvl][vpn] & PG_PRESENT) {
				if (self[lvl][vpn] & PG_USER) {
					size_t phyaddr = get_pages(1);
					if (BUILTIN_EXPECT(!phyaddr, 0))
						return -ENOMEM;

					atomic_int64_inc(dest->user_usage);

					other[lvl][vpn] = phyaddr | (self[lvl][vpn] & ~PAGE_MASK);
					if (lvl) /* PML4, PDPT, PGD */
						traverse(lvl-1, vpn<<PAGE_MAP_BITS); /* Pre-order traversal */
					else { /* PGT */
						page_map(PAGE_TMP, phyaddr, 1, PG_RW);
						memcpy((void*) PAGE_TMP, (void*) (vpn<<PAGE_BITS), PAGE_SIZE);
					}
				}
				else if (self[lvl][vpn] & PG_SELF)
					other[lvl][vpn] = 0;
				else
					other[lvl][vpn] = self[lvl][vpn];
			}
			else
				other[lvl][vpn] = 0;
		}
		return 0;
	}

	spinlock_irqsave_lock(&curr_task->page_lock);
	self[PAGE_LEVELS-1][PAGE_MAP_ENTRIES-2] = dest->page_map | PG_PRESENT | PG_SELF | PG_RW;

	int ret = traverse(PAGE_LEVELS-1, 0);

	other[PAGE_LEVELS-1][PAGE_MAP_ENTRIES-1] = dest->page_map | PG_PRESENT | PG_SELF | PG_RW;
	self [PAGE_LEVELS-1][PAGE_MAP_ENTRIES-2] = 0;
	spinlock_irqsave_unlock(&curr_task->page_lock);

	/* Flush TLB entries of 'other' self-reference */
	flush_tlb();

	return ret;
}

void page_fault_handler(struct state *s)
{
	size_t viraddr = read_cr2();
	task_t* task = per_core(current_task);

	// on demand userspace heap mapping
	if ((task->heap) && (viraddr >= task->heap->start) && (viraddr < task->heap->end)) {
		viraddr &= PAGE_MASK;

		size_t phyaddr = get_page();
		if (BUILTIN_EXPECT(!phyaddr, 0)) {
			kprintf("out of memory: task = %u\n", task->id);
			goto default_handler;
		}

		int ret = page_map(viraddr, phyaddr, 1, PG_USER|PG_RW);
		if (BUILTIN_EXPECT(ret, 0)) {
			kprintf("map_region: could not map %#lx to %#lx, task = %u\n", phyaddr, viraddr, task->id);
			put_page(phyaddr);

			goto default_handler;
		}

		memset((void*) viraddr, 0x00, PAGE_SIZE); // fill with zeros

		return;
	}

default_handler:
	kprintf("Page Fault Exception (%d) at cs:ip = %#x:%#lx, fs = %#lx, gs = %#lx, rflags 0x%lx, task = %u, addr = %#lx, error = %#x [ %s %s %s %s %s ]\n",
		s->int_no, s->cs, s->rip, s->fs, s->gs, s->rflags, task->id, viraddr, s->error,
		(s->error & 0x4) ? "user" : "supervisor",
		(s->error & 0x10) ? "instruction" : "data",
		(s->error & 0x2) ? "write" : ((s->error & 0x10) ? "fetch" : "read"),
		(s->error & 0x1) ? "protection" : "not present",
		(s->error & 0x8) ? "reserved bit" : "\b");
	kprintf("rax %#lx, rbx %#lx, rcx %#lx, rdx %#lx, rbp %#lx, rsp %#lx rdi %#lx, rsi %#lx\n", s->rax, s->rbx, s->rcx, s->rdx, s->rbp, s->rsp, s->rdi, s->rsi);
	if (task->heap)
		kprintf("Heap 0x%llx - 0x%llx\n", task->heap->start, task->heap->end);

	apic_eoi(s->int_no);
	irq_enable();
	abort();
}

int page_init(void)
{
	/* Replace default pagefault handler */
	irq_uninstall_handler(14);
	irq_install_handler(14, page_fault_handler);

	return 0;
}
