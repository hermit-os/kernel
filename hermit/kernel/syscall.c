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
#include <hermit/stdio.h>
#include <hermit/tasks.h>
#include <hermit/errno.h>
#include <hermit/syscall.h>
#include <hermit/spinlock.h>

static int sys_write(int fd, const char* buf, size_t len)
{
	size_t i;

	//TODO: Currently, we ignore the file descriptor

	if (BUILTIN_EXPECT(!buf, 0))
		return -1;

	for(i=0; i<len; i++)
		kputchar(buf[i]);

	return 0;
}

static ssize_t sys_sbrk(int incr)
{
	task_t* task = per_core(current_task);
	vma_t* heap = task->heap;
	ssize_t ret;

	spinlock_lock(&task->vma_lock);

	if (BUILTIN_EXPECT(!heap, 0)) {
		kprintf("sys_sbrk: missing heap!\n");
		abort();
	}

	ret = heap->end;
	heap->end += incr;
	if (heap->end < heap->start)
		heap->end = heap->start;

	// allocation and mapping of new pages for the heap
	// is catched by the pagefault handler

	spinlock_unlock(&task->vma_lock);

	return ret;
}

ssize_t syscall_handler(uint32_t sys_nr, ...)
{
	ssize_t ret = -EINVAL;
	va_list vl;

	check_workqueues();

	va_start(vl, sys_nr);

	switch(sys_nr)
	{
	case __NR_exit:
		sys_exit(va_arg(vl, uint32_t));
		ret = 0;
		break;
	case __NR_write: {
		int fd = va_arg(vl, int);
		const char* buf = va_arg(vl, const char*);
		size_t len = va_arg(vl, size_t);
		ret = sys_write(fd, buf, len);
		break;
	}
	//TODO: Currently, we ignore file descriptors
	case __NR_open:
	case __NR_close:
		ret = 0;
		break;
	case __NR_sbrk: {
		int incr = va_arg(vl, int);

		ret = sys_sbrk(incr);
		break;
	}
	default:
		kprintf("invalid system call: 0x%lx\n", sys_nr);
		ret = -ENOSYS;
		break;
	};

	va_end(vl);

	return ret;
}
