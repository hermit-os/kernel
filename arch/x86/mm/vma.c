/*
 * Copyright (c) 2014, Steffen Vogel, RWTH Aachen University
 *                     All rights reserved.
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

#include <hermit/stdio.h>
#include <hermit/vma.h>
#include <hermit/logging.h>
#include <asm/multiboot.h>

int vma_arch_init(void)
{
	int ret = 0;

	if (mb_info) {
		ret = vma_add((size_t)mb_info & PAGE_MASK, ((size_t)mb_info & PAGE_MASK) + PAGE_SIZE, VMA_READ|VMA_WRITE);
		if (BUILTIN_EXPECT(ret, 0))
			goto out;

		if ((mb_info->flags & MULTIBOOT_INFO_CMDLINE) && cmdline) {
			LOG_INFO("vma_arch_init: map cmdline %p (size 0x%zd)", cmdline, cmdsize);

			size_t i = 0;
			while(((size_t) cmdline + i) < ((size_t) cmdline + cmdsize))
			{
				if ((((size_t)cmdline + i) & PAGE_MASK) != ((size_t) mb_info & PAGE_MASK)) {
					ret = vma_add(((size_t)cmdline + i) & PAGE_MASK, (((size_t)cmdline + i) & PAGE_MASK) + PAGE_SIZE, VMA_READ|VMA_WRITE);
					if (BUILTIN_EXPECT(ret, 0))
						goto out;
				}

				i += PAGE_SIZE;
			}
		}
	}

out:
	return ret;
}
