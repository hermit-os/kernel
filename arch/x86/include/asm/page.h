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
 * @author Steffen Vogel
 * @file arch/x86/include/asm/page.h
 * @brief Paging related functions
 *
 * This file contains the several functions to manage the page tables
 */

#include <hermit/stddef.h>
#include <hermit/stdlib.h>
#include <asm/processor.h>

#ifndef __PAGE_H__
#define __PAGE_H__

/// Page offset bits
#define PAGE_BITS		12
#define PAGE_2M_BITS		21
/// The size of a single page in bytes
#define PAGE_SIZE		( 1L << PAGE_BITS)
/// Mask the page address without page map flags and XD flag
#if 0
#define PAGE_MASK		((~0UL) << PAGE_BITS)
#define PAGE_2M_MASK		(~0UL) << PAGE_2M_BITS)
#else
#define PAGE_MASK		(((~0UL) << PAGE_BITS) & ~PG_XD)
#define PAGE_2M_MASK		(((~0UL) << PAGE_2M_BITS) & ~PG_XD)
#endif

#if 0
/// Total operand width in bits
#define BITS			32
/// Physical address width (we dont support PAE)
#define PHYS_BITS		BITS
/// Linear/virtual address width
#define VIRT_BITS		BITS
/// Page map bits
#define PAGE_MAP_BITS	10
/// Number of page map indirections
#define PAGE_LEVELS		2
#else
/// Total operand width in bits
#define BITS			64
/// Physical address width (maximum value)
#define PHYS_BITS		52
/// Linear/virtual address width
#define VIRT_BITS		48
/// Page map bits
#define PAGE_MAP_BITS	9
/// Number of page map indirections
#define PAGE_LEVELS		4

/** @brief Sign extending a integer
 *
 * @param addr The integer to extend
 * @param bits The width if addr which should be extended
 * @return The extended integer
 */
static inline size_t sign_extend(ssize_t addr, int bits)
{
	int shift = BITS - bits;
	return (addr << shift) >> shift; // sign bit gets copied during arithmetic right shift
}
#endif

/// Make address canonical
#if 0
#define CANONICAL(addr)		(addr) // only for 32 bit paging
#else
#define CANONICAL(addr)		sign_extend(addr, VIRT_BITS)
#endif

/// The number of entries in a page map table
#define PAGE_MAP_ENTRIES	       (1L << PAGE_MAP_BITS)

/// Align to next page
#define PAGE_CEIL(addr)		(((addr) + PAGE_SIZE - 1) & PAGE_MASK)
/// Align to page
#define PAGE_FLOOR(addr)	( (addr)                  & PAGE_MASK)

/// Align to next 2M boundary
#define PAGE_2M_CEIL(addr)	(((addr) + (1L << 21) - 1) & ((~0L) << 21))
/// Align to nex 2M boundary
#define PAGE_2M_FLOOR(addr)	( (addr)                   & ((~0L) << 21))

/// Page is present
#define PG_PRESENT		(1 << 0)
/// Page is read- and writable
#define PG_RW			(1 << 1)
/// Page is addressable from userspace
#define PG_USER			(1 << 2)
/// Page write through is activated
#define PG_PWT			(1 << 3)
/// Page cache is disabled
#define PG_PCD			(1 << 4)
/// Page was recently accessed (set by CPU)
#define PG_ACCESSED		(1 << 5)
/// Page is dirty due to recent write-access (set by CPU)
#define PG_DIRTY		(1 << 6)
/// Huge page: 4MB (or 2MB, 1GB)
#define PG_PSE			(1 << 7)
/// Page attribute table
#define PG_PAT			PG_PSE
#if 1
/* @brief Global TLB entry (Pentium Pro and later)
 *
 * HermitCore is a single-address space operating system
 * => CR3 never changed => The flag isn't required for HermitCore
 */
#define PG_GLOBAL		0
#else
#define PG_GLOBAL		(1 << 8)
#endif
/// This table is a self-reference and should skipped by page_map_copy()
#define PG_SELF			(1 << 9)

/// Disable execution for this page
#define PG_XD			(1L << 63)

#define PG_NX			(has_nx() ? PG_XD : 0)

/** @brief Converts a virtual address to a physical
 *
 * A non mapped virtual address causes a pagefault!
 *
 * @param addr Virtual address to convert
 * @return physical address
 */
size_t virt_to_phys(size_t vir);

/** @brief Initialize paging subsystem
 *
 * This function uses the existing bootstrap page tables (boot_{pgd, pgt})
 * to map required regions (video memory, kernel, etc..).
 * Before calling page_init(), the bootstrap tables contain a simple identity
 * paging. Which is replaced by more specific mappings.
 */
int page_init(void);

/** @brief Map a continuous region of pages
 *
 * @param viraddr Desired virtual address
 * @param phyaddr Physical address to map from
 * @param npages The region's size in number of pages
 * @param bits Further page flags
 * @param do_ipi if set, inform via IPI all other cores
 * @return
 */
int __page_map(size_t viraddr, size_t phyaddr, size_t npages, size_t bits, uint8_t do_ipi);

/** @brief Map a continuous region of pages
 *
 * @param viraddr Desired virtual address
 * @param phyaddr Physical address to map from
 * @param npages The region's size in number of pages
 * @param bits Further page flags
 * @return
 */
static inline int page_map(size_t viraddr, size_t phyaddr, size_t npages, size_t bits)
{
	return __page_map(viraddr, phyaddr, npages, bits, 1);
}

/** @brief Unmap a continuous region of pages
 *
 * @param viraddr The virtual start address
 * @param npages The range's size in pages
 * @return
 */
int page_unmap(size_t viraddr, size_t npages);

/** @brief Change the page permission in the page tables of the current task
 *
 * Applies given flags noted in the 'flags' parameter to
 * the range denoted by virtual start and end addresses.
 *
 * @param start Range's virtual start address
 * @param end Range's virtual end address
 * @param flags flags to apply
 *
 * @return
 * - 0 on success
 * - -EINVAL (-22) on failure.
 */
int page_set_flags(size_t viraddr, uint32_t npages, int flags);

#endif
