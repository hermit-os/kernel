/*
 * Copyright (c) 2011, Stefan Lankes, RWTH Aachen University
 *               2014, Steffen Vogel, RWTH Aachen University
 *               All rights reserved.
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
 * @author Steffen Vogel <steffen.vogel@rwth-aachen.de>
 * @file include/hermit/vma.h
 * @brief VMA related sructure and functions
 */

#ifndef __VMA_H__
#define __VMA_H__

#include <hermit/stddef.h>
#include <asm/page.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Read access to this VMA is allowed
#define VMA_READ	(1 << 0)
/// Write access to this VMA is allowed
#define VMA_WRITE	(1 << 1)
/// Instructions fetches in this VMA are allowed
#define VMA_EXECUTE	(1 << 2)
/// This VMA is cacheable
#define VMA_CACHEABLE	(1 << 3)
/// This VMA is not accessable
#define VMA_NO_ACCESS	(1 << 4)
/// This VMA should be part of the userspace
#define VMA_USER	(1 << 5)
/// A collection of flags used for the kernel heap (kmalloc)
#define VMA_HEAP	(VMA_READ|VMA_WRITE|VMA_CACHEABLE)

// boundaries for VAS allocation
#define VMA_MIN	0xC0000

// last three top level entries are reserved
#define VMA_MAX	0xFFFFFE8000000000

struct vma;

/** @brief VMA structure definition
 *
 * Each item in this linked list marks a used part of the virtual address space.
 * Its used by vm_alloc() to find holes between them.
 */
typedef struct vma {
	/// Start address of the memory area
	size_t start;
	/// End address of the memory area
	size_t end;
	/// Type flags field
	uint32_t flags;
	/// Pointer of next VMA element in the list
	struct vma* next;
	/// Pointer to previous VMA element in the list
	struct vma* prev;
} vma_t;

/** @brief Initalize the kernelspace VMA list
 *
 * Reserves several system-relevant virtual memory regions:
 *  - The kernel (image_size)
 *  - Init Ramdisk
 *
 * @return
 *  - 0 on success
 *  - <0 on failure
 */
int vma_init(void);

/** @brief Initalize the kernelspace VMA list
 *
 * Reserves several architecture-relevant virtual memory regions
 *
 * @return
 *  - 0 on success
 *  - <0 on failure
 */
int vma_arch_init(void);

/** @brief Add a new virtual memory area to the list of VMAs
 *
 * @param start Start address of the new area
 * @param end End address of the new area
 * @param flags Type flags the new area shall have
 *
 * @return
 * - 0 on success
 * - -EINVAL (-22) or -EINVAL (-12) on failure
 */
int vma_add(size_t start, size_t end, uint32_t flags);

/** @brief Search for a free memory area
 *
 * @param size Size of requestes VMA in bytes
 * @param flags
 * @return Type flags the new area shall have
 * - 0 on failure
 * - the start address of a free area
 */
size_t vma_alloc(size_t size, uint32_t flags);

/** @brief Free an allocated memory area
 *
 * @param start Start address of the area to be freed
 * @param end End address of the to be freed
 * @return
 * - 0 on success
 * - -EINVAL (-22) on failure
 */
int vma_free(size_t start, size_t end);

/** @brief Dump information about this task's VMAs into the terminal. */
void vma_dump(void);

#ifdef __cplusplus
}
#endif

#endif
