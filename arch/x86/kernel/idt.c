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
 * @author Stefan Lankes
 * @file arch/x86/kernel/idt.c
 * @brief Definitions and functions related to IDT
 *
 *
 * This file defines the interface for interrupts as like 
 * structures to describe interrupt descriptor table entries.\n
 * See idt.h for flag definitions.
 */

#include <hermit/string.h>
#include <asm/idt.h>

/* 
 * Declare an IDT of 256 entries. Although we will only use the
 * first 32 entries in this tutorial, the rest exists as a bit
 * of a trap. If any undefined IDT entry is hit, it normally
 * will cause an "Unhandled Interrupt" exception. Any descriptor
 * for which the 'presence' bit is cleared (0) will generate an
 * "Unhandled Interrupt" exception 
 */
static idt_entry_t idt[256] = {[0 ... 255] = {0, 0, 0, 0, 0, 0, 0}};
static idt_ptr_t idtp;

static void configure_idt_entry(idt_entry_t *dest_entry, size_t base, uint16_t sel, uint8_t flags, uint8_t idx)
{
	/* The interrupt routine's base address */
	dest_entry->base_lo = (base & 0xFFFF);
	dest_entry->base_hi = (base >> 16) & 0xFFFF;

	/* The segment or 'selector' that this IDT entry will use
	 *  is set here, along with any access flags */
	dest_entry->sel = sel;
	dest_entry->ist_index = idx;
	dest_entry->flags = flags;
}

/*
 * Use this function to set an entry in the IDT. Alot simpler
 * than twiddling with the GDT ;)
 */
void idt_set_gate(uint8_t num, size_t base, uint16_t sel, uint8_t flags, uint8_t idx)
{
	configure_idt_entry(&idt[num], base, sel, flags, idx);
}

#if 0
extern void int80_syscall(void);
#endif

/* Installs the IDT */
void idt_install(void)
{
	static int initialized = 0;

	if (!initialized) {
		initialized = 1;

		/* Sets the special IDT pointer up, just like in 'gdt.c' */
		idtp.limit = (sizeof(idt_entry_t) * 256) - 1;
		idtp.base = (size_t)&idt;

#if 0
		/* Add any new ISRs to the IDT here using idt_set_gate */
		idt_set_gate(INT_SYSCALL, (size_t)int80_syscall, KERNEL_CODE_SELECTOR,
			IDT_FLAG_PRESENT|IDT_FLAG_RING3|IDT_FLAG_32BIT|IDT_FLAG_TRAPGATE);
#endif
	}

	/* Points the processor's internal register to the new IDT */
	asm volatile("lidt %0" : : "m" (idtp));
}
