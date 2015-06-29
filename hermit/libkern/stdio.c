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
#include <asm/multiboot.h>
#ifdef CONFIG_VGA
#include <asm/vga.h>
#endif

#define NO_EARLY_PRINT		0x00
#define VGA_EARLY_PRINT		0x01

#ifdef CONFIG_VGA
static uint32_t early_print = VGA_EARLY_PRINT;
#else
static uint32_t early_print = NO_EARLY_PRINT;
#endif
static spinlock_irqsave_t olock = SPINLOCK_IRQSAVE_INIT;
static atomic_int32_t kmsg_counter = ATOMIC_INIT(-1);
static unsigned char kmessages[KMSG_SIZE] __attribute__ ((section(".kmsg"))) = {[0 ... KMSG_SIZE-1] = 0x00};

int koutput_init(void)
{
#ifdef CONFIG_VGA
	vga_init();
#endif

	return 0;
}

int kputchar(int c)
{
	int pos;

	/* add place holder for end of string */
	if (!c)
		c = '?';

	if (early_print != NO_EARLY_PRINT)
		spinlock_irqsave_lock(&olock);

	pos = atomic_int32_inc(&kmsg_counter);
	kmessages[pos % KMSG_SIZE] = (unsigned char) c;

#ifdef CONFIG_VGA
	if (early_print & VGA_EARLY_PRINT)
		vga_putchar(c);
#endif

	if (early_print != NO_EARLY_PRINT)
		spinlock_irqsave_unlock(&olock);

	return 1;
}

int kputs(const char *str)
{
	int pos, i, len = strlen(str);

	if (early_print != NO_EARLY_PRINT)
		spinlock_irqsave_lock(&olock);

	for(i=0; i<len; i++) {
		pos = atomic_int32_inc(&kmsg_counter);
		kmessages[pos % KMSG_SIZE] = str[i];
#ifdef CONFIG_VGA
		if (early_print & VGA_EARLY_PRINT)
			vga_putchar(str[i]);
#endif
	}

	if (early_print != NO_EARLY_PRINT)
		spinlock_irqsave_unlock(&olock);

	return len;
}
