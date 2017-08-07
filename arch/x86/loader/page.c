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

#include <stdio.h>
#include <string.h>
#include <page.h>
#include <multiboot.h>

/* Note that linker symbols are not variables, they have no memory
 * allocated for maintaining a value, rather their address is their value. */
extern const void kernel_start;
extern const void kernel_end;

/// This page is reserved for copying
#define PAGE_TMP		(PAGE_CEIL((size_t) &kernel_start) - PAGE_SIZE)

/** This PGD table is initialized in entry.asm */
extern size_t* boot_map;

#ifdef CONFIG_X86_32
/** A self-reference enables direct access to all page tables */
static size_t * const self[PAGE_LEVELS] = {
	(size_t *) 0xFFC00000,
	(size_t *) 0xFFFFF000
};
#elif defined(CONFIG_X86_64)
/** A self-reference enables direct access to all page tables */
static size_t* const self[PAGE_LEVELS] = {
	(size_t *) 0xFFFFFF8000000000,
	(size_t *) 0xFFFFFFFFC0000000,
	(size_t *) 0xFFFFFFFFFFE00000,
	(size_t *) 0xFFFFFFFFFFFFF000
};
#endif

/** @brief Flush a specific page entry in TLB
 *  @param addr The (virtual) address of the page to flush
 */
static inline void tlb_flush_one_page(size_t addr)
{
	asm volatile("invlpg (%0)" : : "r"(addr) : "memory");
}

size_t virt_to_phys(size_t addr)
{
	size_t vpn   = addr >> PAGE_BITS;	// virtual page number
	size_t entry = self[0][vpn];		// page table entry
	size_t off   = addr  & ~PAGE_MASK;	// offset within page
	size_t phy   = entry &  PAGE_MASK;	// physical page frame number

	return phy | off;
}

static  size_t first_page = (size_t) &kernel_start - PAGE_SIZE;

size_t get_page(void)
{
	size_t ret = first_page;

	first_page += PAGE_SIZE;

	return ret;
}

int page_map(size_t viraddr, size_t phyaddr, size_t npages, size_t bits)
{
	int lvl, ret = -1;
	long vpn = viraddr >> PAGE_BITS;
	long first[PAGE_LEVELS], last[PAGE_LEVELS];

	/* Calculate index boundaries for page map traversal */
	for (lvl=0; lvl<PAGE_LEVELS; lvl++) {
		first[lvl] = (vpn         ) >> (lvl * PAGE_MAP_BITS);
		last[lvl]  = (vpn+npages-1) >> (lvl * PAGE_MAP_BITS);
	}

	/* Start iterating through the entries
	 * beginning at the root table (PGD or PML4) */
	for (lvl=PAGE_LEVELS-1; lvl>=0; lvl--) {
		for (vpn=first[lvl]; vpn<=last[lvl]; vpn++) {
			if (lvl) { /* PML4, PDPT, PGD */
				if (!(self[lvl][vpn] & PG_PRESENT)) {
					/* There's no table available which covers the region.
					 * Therefore we need to create a new empty table. */
					size_t phyaddr = get_page();
					if (BUILTIN_EXPECT(!phyaddr, 0))
						goto out;

					/* Reference the new table within its parent */
#ifdef CONFIG_X86_32
					self[lvl][vpn] = phyaddr | bits | PG_PRESENT | PG_USER | PG_RW;
#elif defined(CONFIG_X86_64)					
					self[lvl][vpn] = (phyaddr | bits | PG_PRESENT | PG_USER | PG_RW) & ~PG_XD;
#endif

					/* Fill new table with zeros */
					memset(&self[lvl-1][vpn<<PAGE_MAP_BITS], 0, PAGE_SIZE);
				}
			}
			else { /* PGT */
				if (self[lvl][vpn] & PG_PRESENT)
					/* There's already a page mapped at this address.
					 * We have to flush a single TLB entry. */
					tlb_flush_one_page(vpn << PAGE_BITS);

				self[lvl][vpn] = phyaddr | bits | PG_PRESENT;
				phyaddr += PAGE_SIZE;
			}
		}
	}

	ret = 0;
out:

	return ret;
}

/** Tables are freed by page_map_drop() */
int page_unmap(size_t viraddr, size_t npages)
{
	/* We aquire both locks for kernel and task tables
	 * as we dont know to which the region belongs. */

	/* Start iterating through the entries.
	 * Only the PGT entries are removed. Tables remain allocated. */
	size_t vpn, start = viraddr>>PAGE_BITS;
	for (vpn=start; vpn<start+npages; vpn++)
		self[0][vpn] = 0;


	/* This can't fail because we don't make checks here */
	return 0;
}

int page_init(void)
{
	/* Map multiboot information and modules */
	if (mb_info) {
		size_t addr, npages;
		int ret;

		// already mapped => entry.asm
		//addr = (size_t) mb_info & PAGE_MASK;
		//npages = PAGE_CEIL(sizeof(*mb_info)) >> PAGE_BITS;
		//page_map(addr, addr, npages, PG_GLOBAL);

		if (mb_info->flags & MULTIBOOT_INFO_MODS) {
			addr = mb_info->mods_addr;
			npages = PAGE_CEIL(mb_info->mods_count*sizeof(multiboot_module_t)) >> PAGE_BITS;
			ret = page_map(addr, addr, npages, PG_GLOBAL);
			kprintf("Map module info at 0x%lx (ret %d)\n", addr, ret);

			multiboot_module_t* mmodule = (multiboot_module_t*) ((size_t) mb_info->mods_addr);

			// at first we determine the first free page
			for(int i=0; i<mb_info->mods_count; i++) {
				if (first_page < mmodule[i].mod_end)
					first_page = PAGE_CEIL(mmodule[i].mod_end);
			}

			// we map only the first page of each module (= ELF file) because
			// we need only the program header of the ELF file
			for(int i=0; i<mb_info->mods_count; i++) {
				addr = mmodule[i].mod_start;
				npages = PAGE_CEIL(mmodule[i].mod_end - mmodule[i].mod_start) >> PAGE_BITS;
				ret = page_map(addr, addr, 1 /*npages*/, PG_GLOBAL);
				kprintf("Map first page of module %d at 0x%lx (ret %d)\n", i, addr, ret);
				kprintf("Module %d consists %zd\n", i, npages);
			}
		}
	}

	// add space for the migration of the elf file
	first_page += 0x200000;
	kprintf("Page pool starts at 0x%zx\n", first_page);

	return 0;
}
