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

/**
 * @author Steffen Vogel
 * @file include/memory.h
 * @brief Memory related functions
 *
 * This file contains platform independent memory functions
 */

#ifndef __MEMORY_H__
#define __MEMORY_H__

/** @brief Initialize the memory subsystem */
int memory_init(void);

/** @brief Request physical page frames */
size_t get_pages(size_t npages);

/** @brief Get a single page
 *
 * Convenience function: uses get_pages(1);
 */
static inline size_t get_page(void) { return get_pages(1); }

/** @brief release physical page frames */
int put_pages(size_t phyaddr, size_t npages);

/** @brief Put a single page
 *
 * Convenience function: uses put_pages(1);
 */
static inline int put_page(size_t phyaddr) { return put_pages(phyaddr, 1); }

/** @brief Copy a physical page frame
 *
 * @param psrc physical address of source page frame
 * @param pdest physical address of source page frame
 * @return
 * - 0 on success
 * - -1 on failure
 */
int copy_page(size_t pdest, size_t psrc);

#endif
