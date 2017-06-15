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
#include <hermit/logging.h>

#include <asm/multiboot.h>
#include <asm/irq.h>
#include <asm/page.h>

/* Note that linker symbols are not variables, they have no memory
 * allocated for maintaining a value, rather their address is their value. */
extern const void kernel_start;

/// This page is reserved for copying
#define PAGE_TMP		(PAGE_FLOOR((size_t) &kernel_start) - PAGE_SIZE)

/** Single-address space operating system => one lock for all tasks */
static spinlock_irqsave_t page_lock = SPINLOCK_IRQSAVE_INIT;

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

#if 0
/** An other self-reference for page_map_copy() */
static size_t * const other[PAGE_LEVELS] = {
	(size_t *) 0xFFFFFF0000000000,
	(size_t *) 0xFFFFFFFF80000000,
	(size_t *) 0xFFFFFFFFFFC00000,
	(size_t *) 0xFFFFFFFFFFFFE000
};
#endif
#endif

static uint8_t expect_zeroed_pages = 0;

size_t virt_to_phys(size_t addr)
{
	if ((addr > (size_t) &kernel_start) &&
	    (addr <= PAGE_2M_FLOOR((size_t) &kernel_start + image_size)))
	{
		size_t vpn   = addr >> (PAGE_2M_BITS);	// virtual page number
		size_t entry = self[1][vpn];		// page table entry
		size_t off   = addr  & ~PAGE_2M_MASK;	// offset within page
		size_t phy   = entry &  PAGE_2M_MASK;	// physical page frame number

		return phy | off;

	} else {
		size_t vpn   = addr >> PAGE_BITS;	// virtual page number
		size_t entry = self[0][vpn];		// page table entry
		size_t off   = addr  & ~PAGE_MASK;	// offset within page
		size_t phy   = entry &  PAGE_MASK;	// physical page frame number

		return phy | off;
	}
}

/*
 * get memory page size
 */
int getpagesize(void)
{
	return PAGE_SIZE;
}

//TODO: code is missing
int page_set_flags(size_t viraddr, uint32_t npages, int flags)
{
	return -EINVAL;
}

int __page_map(size_t viraddr, size_t phyaddr, size_t npages, size_t bits, uint8_t do_ipi)
{
	int lvl, ret = -ENOMEM;
	long vpn = viraddr >> PAGE_BITS;
	long first[PAGE_LEVELS], last[PAGE_LEVELS];
	int8_t send_ipi = 0;

	//kprintf("Map %d pages at 0x%zx\n", npages, viraddr);

	/* Calculate index boundaries for page map traversal */
	for (lvl=0; lvl<PAGE_LEVELS; lvl++) {
		first[lvl] = (vpn         ) >> (lvl * PAGE_MAP_BITS);
		last[lvl]  = (vpn+npages-1) >> (lvl * PAGE_MAP_BITS);
	}

	spinlock_irqsave_lock(&page_lock);

	/* Start iterating through the entries
	 * beginning at the root table (PGD or PML4) */
	for (lvl=PAGE_LEVELS-1; lvl>=0; lvl--) {
		for (vpn=first[lvl]; vpn<=last[lvl]; vpn++) {
			if (lvl) { /* PML4, PDPT, PGD */
				if (!(self[lvl][vpn] & PG_PRESENT)) {
					/* There's no table available which covers the region.
					 * Therefore we need to create a new empty table. */
					size_t paddr = get_pages(1);
					if (BUILTIN_EXPECT(!paddr, 0))
						goto out;

					/* Reference the new table within its parent */
#if 0
					self[lvl][vpn] = paddr | bits | PG_PRESENT | PG_USER | PG_RW | PG_ACCESSED | PG_DIRTY;
#else
					self[lvl][vpn] = (paddr | bits | PG_PRESENT | PG_USER | PG_RW | PG_ACCESSED | PG_DIRTY) & ~PG_XD;
#endif

					/* Fill new table with zeros */
					memset(&self[lvl-1][vpn<<PAGE_MAP_BITS], 0, PAGE_SIZE);
				}
			}
			else { /* PGT */
				int8_t flush = 0;

				/* do we have to flush the TLB? */
				if (self[lvl][vpn] & PG_PRESENT) {
					//kprintf("Remap address 0x%zx at core %d\n", viraddr, CORE_ID);
					send_ipi = flush = 1;
				}

				self[lvl][vpn] = phyaddr | bits | PG_PRESENT | PG_ACCESSED | PG_DIRTY;

				if (flush)
					/* There's already a page mapped at this address.
					 * We have to flush a single TLB entry. */
					tlb_flush_one_page(vpn << PAGE_BITS, 0);

				phyaddr += PAGE_SIZE;
				//viraddr += PAGE_SIZE;
			}
		}
	}

	if (do_ipi && send_ipi)
		ipi_tlb_flush();

	ret = 0;
out:
	spinlock_irqsave_unlock(&page_lock);

	return ret;
}

int page_unmap(size_t viraddr, size_t npages)
{
	if (BUILTIN_EXPECT(!npages, 0))
		return 0;

	//kprintf("Unmap %d pages at 0x%zx\n", npages, viraddr);

	spinlock_irqsave_lock(&page_lock);

	/* Start iterating through the entries.
	 * Only the PGT entries are removed. Tables remain allocated. */
	size_t vpn, start = viraddr>>PAGE_BITS;
	for (vpn=start; vpn<start+npages; vpn++) {
		self[0][vpn] = 0;
		tlb_flush_one_page(vpn << PAGE_BITS, 0);
	}

	ipi_tlb_flush();

	spinlock_irqsave_unlock(&page_lock);

	/* This can't fail because we don't make checks here */
	return 0;
}

void page_fault_handler(struct state *s)
{
	size_t viraddr = read_cr2();
	task_t* task = per_core(current_task);

	int check_pagetables(size_t vaddr)
	{
		int lvl;
		long vpn = vaddr >> PAGE_BITS;
		long index[PAGE_LEVELS];

		/* Calculate index boundaries for page map traversal */
		for (lvl=0; lvl<PAGE_LEVELS; lvl++)
			index[lvl] = vpn >> (lvl * PAGE_MAP_BITS);

		/* do we have already a valid entry in the page tables */
		for (lvl=PAGE_LEVELS-1; lvl>=0; lvl--) {
			vpn = index[lvl];

			if (!(self[lvl][vpn] & PG_PRESENT))
				return 0;
		}

		return 1;
	}

	spinlock_irqsave_lock(&page_lock);

	if ((task->heap) && (viraddr >= task->heap->start) && (viraddr < task->heap->end)) {
		size_t flags;
		int ret;

		/*
		 * do we have a valid page table entry? => flush TLB and return
		 */
		if (check_pagetables(viraddr)) {
			//tlb_flush_one_page(viraddr);
			spinlock_irqsave_unlock(&page_lock);
			return;
		}

		 // on demand userspace heap mapping
		viraddr &= PAGE_MASK;

		size_t phyaddr = expect_zeroed_pages ? get_zeroed_page() : get_page();
		if (BUILTIN_EXPECT(!phyaddr, 0)) {
			LOG_ERROR("out of memory: task = %u\n", task->id);
			goto default_handler;
		}

		flags = PG_USER|PG_RW;
		if (has_nx()) // set no execution flag to protect the heap
			flags |= PG_XD;
		ret = __page_map(viraddr, phyaddr, 1, flags, 0);

		if (BUILTIN_EXPECT(ret, 0)) {
			LOG_ERROR("map_region: could not map %#lx to %#lx, task = %u\n", phyaddr, viraddr, task->id);
			put_page(phyaddr);

			goto default_handler;
		}

		spinlock_irqsave_unlock(&page_lock);

		return;
	}

default_handler:
	spinlock_irqsave_unlock(&page_lock);

	LOG_ERROR("Page Fault Exception (%d) on core %d at cs:ip = %#x:%#lx, fs = %#lx, gs = %#lx, rflags 0x%lx, task = %u, addr = %#lx, error = %#x [ %s %s %s %s %s ]\n",
		s->int_no, CORE_ID, s->cs, s->rip, s->fs, s->gs, s->rflags, task->id, viraddr, s->error,
		(s->error & 0x4) ? "user" : "supervisor",
		(s->error & 0x10) ? "instruction" : "data",
		(s->error & 0x2) ? "write" : ((s->error & 0x10) ? "fetch" : "read"),
		(s->error & 0x1) ? "protection" : "not present",
		(s->error & 0x8) ? "reserved bit" : "\b");
	LOG_ERROR("rax %#lx, rbx %#lx, rcx %#lx, rdx %#lx, rbp, %#lx, rsp %#lx rdi %#lx, rsi %#lx, r8 %#lx, r9 %#lx, r10 %#lx, r11 %#lx, r12 %#lx, r13 %#lx, r14 %#lx, r15 %#lx\n",
		s->rax, s->rbx, s->rcx, s->rdx, s->rbp, s->rsp, s->rdi, s->rsi, s->r8, s->r9, s->r10, s->r11, s->r12, s->r13, s->r14, s->r15);
	if (task->heap)
		LOG_ERROR("Heap 0x%llx - 0x%llx\n", task->heap->start, task->heap->end);

	apic_eoi(s->int_no);
	//do_abort();
	sys_exit(-EFAULT);
}

// weak symbol is used to detect a Go application
void __attribute__((weak)) runtime_osinit();

int page_init(void)
{
	// do we have Go application? => weak symbol isn't zeroe
	// => Go expect zeroed pages => set zeroed_pages to true
	if (runtime_osinit) {
		expect_zeroed_pages = 1;
		LOG_INFO("Detect Go runtime! Consequently, HermitCore zeroed heap.\n");
	}

	if (mb_info && (mb_info->flags & MULTIBOOT_INFO_CMDLINE) && (cmdline))
	{
		size_t i = 0;

		while(((size_t) cmdline + i) <= ((size_t) cmdline + cmdsize))
		{
			page_map(((size_t) cmdline + i) & PAGE_MASK, ((size_t) cmdline + i) & PAGE_MASK, 1, PG_GLOBAL|PG_RW|PG_PRESENT);
			i += PAGE_SIZE;
		}
	} else cmdline = 0;

	/* Replace default pagefault handler */
	irq_uninstall_handler(14);
	irq_install_handler(14, page_fault_handler);

	return 0;
}
