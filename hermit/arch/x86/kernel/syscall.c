/*
 * Copyright (c) 2016, Stefan Lankes, RWTH Aachen University
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
#include <hermit/logging.h>

void __startcontext(void);

void makecontext(ucontext_t *ucp, void (*func)(), int argc, ...)
{
	va_list ap;

	if (BUILTIN_EXPECT(!ucp, 0))
		return;

	LOG_DEBUG("sys_makecontext %p, func %p, stack 0x%zx, task %d\n", ucp, func, ucp->uc_stack.ss_sp, per_core(current_task)->id);

	size_t* stack = (size_t*) ((size_t)ucp->uc_stack.ss_sp + ucp->uc_stack.ss_size);
	stack -= (argc > 6 ? argc - 6 : 0) + 1;
	uint32_t idx = (argc > 6 ? argc - 6 : 0) + 1;

	/* Align stack and reserve space for trampoline address.  */
	stack = (size_t*) ((((size_t) stack) & ~0xFULL) - 0x8);

	/* Setup context */
	ucp->uc_mregs.rip = (size_t) func;
	ucp->uc_mregs.rbx = (size_t) &stack[idx];
	ucp->uc_mregs.rsp = (size_t) stack;

	stack[0] = (size_t) &__startcontext;
	stack[idx] = (size_t) ucp->uc_link; // link to the next context

	va_start(ap, argc);
	for (int i = 0; i < argc; i++)
	{
		switch (i)
		{
		case 0:
			ucp->uc_mregs.rdi = va_arg(ap, size_t);
			break;
		case 1:
			ucp->uc_mregs.rsi = va_arg(ap, size_t);
			break;
		case 2:
			ucp->uc_mregs.rdx = va_arg(ap, size_t);
			break;
		case 3:
			ucp->uc_mregs.rcx = va_arg(ap, size_t);
			break;
		case 4:
			ucp->uc_mregs.r8 = va_arg(ap, size_t);
			break;
		case 5:
			ucp->uc_mregs.r9 = va_arg(ap, size_t);
			break;
		default:
			/* copy value on stack */
			stack[i - 5] = va_arg(ap, size_t);
			break;
		}
	}
	va_end(ap);
}

int swapcontext(ucontext_t *oucp, const ucontext_t *ucp)
{
	//TODO: implementation is missing

	LOG_WARNING("sys_swapcontext is currently not implemented: %p <=> %p\n", oucp, ucp);
	return -ENOSYS;
}
