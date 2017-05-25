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

#include <hermit/stdio.h>
#include <hermit/stdlib.h>
#include <hermit/string.h>
#include <hermit/stdarg.h>
#include <hermit/spinlock.h>
#include <asm/atomic.h>
#include <asm/processor.h>
#include <asm/uart.h>

static atomic_int32_t kmsg_counter = ATOMIC_INIT(-1);
spinlock_irqsave_t stdio_lock = SPINLOCK_IRQSAVE_INIT;

/* Workaround for a compiler bug. gcc 5.1 seems to ignore this array, if we
   defined it as as static array. At least it is as static array not part of
   the binary. => no valid kernel messages */
/* static */ unsigned char kmessages[KMSG_SIZE+1] __attribute__ ((section(".kmsg"))) = {[0 ... KMSG_SIZE] = 0x00};

int koutput_init(void)
{
	if (is_single_kernel())
		uart_init();

	return 0;
}

int kputchar(int c)
{
	int pos;

	/* add place holder for end of string */
	if (BUILTIN_EXPECT(!c, 0))
		c = '?';

	pos = atomic_int32_inc(&kmsg_counter);
	kmessages[pos % KMSG_SIZE] = (unsigned char) c;

	if (is_single_kernel())
		uart_putchar(c);

	return 1;
}

int kputs(const char *str)
{
	int pos, i, len = strlen(str);

	spinlock_irqsave_lock(&stdio_lock);

	for(i=0; i<len; i++) {
		pos = atomic_int32_inc(&kmsg_counter);
		kmessages[pos % KMSG_SIZE] = str[i];
	}

	if (is_single_kernel())
		uart_puts(str);

	spinlock_irqsave_unlock(&stdio_lock);

	return len;
}
