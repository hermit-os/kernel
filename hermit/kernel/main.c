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
#include <hermit/string.h>
#include <hermit/time.h>
#include <hermit/tasks.h>
#include <hermit/processor.h>
#include <hermit/tasks.h>
#include <hermit/syscall.h>
#include <hermit/memory.h>

#include <asm/irq.h>
#include <asm/atomic.h>
#include <asm/page.h>

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void kernel_start;
extern const void kernel_end;
extern const void bss_start;
extern const void bss_end;
extern char __BUILD_DATE;
extern char __BUILD_TIME;

/* Page frame counters */
extern atomic_int32_t total_pages;
extern atomic_int32_t total_allocated_pages;
extern atomic_int32_t total_available_pages;

static int hermit_init(void)
{
	// initialize .bss section
	memset((void*)&bss_start, 0x00, ((size_t) &bss_end - (size_t) &bss_start));

	koutput_init();
	system_init();
	irq_init();
	timer_init();
	multitasking_init();
	memory_init();

	return 0;
}

int main(void)
{
	hermit_init();
	system_calibration(); // enables also interrupts

	kprintf("This is Hermit %s Build %u, %u\n", HERMIT_VERSION, &__BUILD_DATE, &__BUILD_TIME);
	kprintf("Kernel starts at %p and ends at %p\n", &kernel_start, &kernel_end);
	kprintf("Processor frequency: %u MHz\n", get_cpu_frequency());
	kprintf("Total memory: %lu KiB\n", atomic_int32_read(&total_pages) * PAGE_SIZE / 1024);
	kprintf("Current allocated memory: %lu KiB\n", atomic_int32_read(&total_allocated_pages) * PAGE_SIZE / 1024);
	kprintf("Current available memory: %lu KiB\n", atomic_int32_read(&total_available_pages) * PAGE_SIZE / 1024);

	while(1) { 
		HALT;
	}

	return 0;
}
