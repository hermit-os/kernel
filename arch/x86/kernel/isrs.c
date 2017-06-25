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
 * @file arch/x86/kernel/isrs.c
 * @brief Installation of interrupt service routines and definition of fault handler.
 *
 * This file contains prototypes for the first 32 entries of the IDT,
 * an ISR installer procedure and a fault handler.\n
 */

#include <hermit/stdio.h>
#include <hermit/tasks.h>
#include <hermit/errno.h>
#include <hermit/logging.h>
#include <asm/irqflags.h>
#include <asm/isrs.h>
#include <asm/irq.h>
#include <asm/idt.h>
#include <asm/apic.h>

/*
 * These are function prototypes for all of the exception
 * handlers: The first 32 entries in the IDT are reserved
 * by Intel and are designed to service exceptions!
 */
extern void isr0(void);
extern void isr1(void);
extern void isr2(void);
extern void isr3(void);
extern void isr4(void);
extern void isr5(void);
extern void isr6(void);
extern void isr7(void);
extern void isr8(void);
extern void isr9(void);
extern void isr10(void);
extern void isr11(void);
extern void isr12(void);
extern void isr13(void);
extern void isr14(void);
extern void isr15(void);
extern void isr16(void);
extern void isr17(void);
extern void isr18(void);
extern void isr19(void);
extern void isr20(void);
extern void isr21(void);
extern void isr22(void);
extern void isr23(void);
extern void isr24(void);
extern void isr25(void);
extern void isr26(void);
extern void isr27(void);
extern void isr28(void);
extern void isr29(void);
extern void isr30(void);
extern void isr31(void);

static void arch_fault_handler(struct state *s);
static void arch_fpu_handler(struct state *s);
extern void fpu_handler(void);

/*
 * This is a very repetitive function... it's not hard, it's
 * just annoying. As you can see, we set the first 32 entries
 * in the IDT to the first 32 ISRs. We can't use a for loop
 * for this, because there is no way to get the function names
 * that correspond to that given entry. We set the access
 * flags to 0x8E. This means that the entry is present, is
 * running in ring 0 (kernel level), and has the lower 5 bits
 * set to the required '14', which is represented by 'E' in
 * hex.
 */
void isrs_install(void)
{
	int i;

	/*
	 * "User-level" doesn't protect the red zone. Consequently we
	 * protect the common stack by the usage of IST number 1.
	 */
	idt_set_gate(0, (size_t)isr0, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(1, (size_t)isr1, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	// NMI Exception gets its own stack (ist2)
	idt_set_gate(2, (size_t)isr2, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 2);
	idt_set_gate(3, (size_t)isr3, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(4, (size_t)isr4, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(5, (size_t)isr5, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(6, (size_t)isr6, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(7, (size_t)isr7, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	// Double Fault Exception gets its own stack (ist3)
	idt_set_gate(8, (size_t)isr8, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 3);
	idt_set_gate(9, (size_t)isr9, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(10, (size_t)isr10, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(11, (size_t)isr11, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(12, (size_t)isr12, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(13, (size_t)isr13, KERNEL_CODE_SELECTOR,
		 IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(14, (size_t)isr14, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(15, (size_t)isr15, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(16, (size_t)isr16, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(17, (size_t)isr17, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	// Machine Check Exception gets its own stack (ist4)
	idt_set_gate(18, (size_t)isr18, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 4);
	idt_set_gate(19, (size_t)isr19, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(20, (size_t)isr20, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(21, (size_t)isr21, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(22, (size_t)isr22, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(23, (size_t)isr23, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(24, (size_t)isr24, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(25, (size_t)isr25, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(26, (size_t)isr26, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(27, (size_t)isr27, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(28, (size_t)isr28, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(29, (size_t)isr29, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(30, (size_t)isr30, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);
	idt_set_gate(31, (size_t)isr31, KERNEL_CODE_SELECTOR,
		IDT_FLAG_PRESENT|IDT_FLAG_RING0|IDT_FLAG_32BIT|IDT_FLAG_INTTRAP, 1);

	// install the default handler
	for(i=0; i<32; i++)
		irq_install_handler(i, arch_fault_handler);

	// set hanlder for fpu exceptions
	irq_uninstall_handler(7);
	irq_install_handler(7, arch_fpu_handler);
}

/** @brief Exception messages
 *
 * This is a simple string array. It contains the message that
 * corresponds to each and every exception. We get the correct
 * message by accessing it like this:
 * exception_message[interrupt_number]
 */
static const char *exception_messages[] = {
	"Division By Zero", "Debug", "Non Maskable Interrupt",
	"Breakpoint", "Into Detected Overflow", "Out of Bounds", "Invalid Opcode",
	"No Coprocessor", "Double Fault", "Coprocessor Segment Overrun", "Bad TSS",
	"Segment Not Present", "Stack Fault", "General Protection Fault", "Page Fault",
	"Unknown Interrupt", "Coprocessor Fault", "Alignment Check", "Machine Check",
	"SIMD Floating-Point", "Virtualization", "Reserved", "Reserved", "Reserved",
	"Reserved", "Reserved", "Reserved", "Reserved", "Reserved", "Reserved",
	"Reserved", "Reserved" };

/* interrupt handler to save / restore the FPU context */
static void arch_fpu_handler(struct state *s)
{
	(void) s;

	clts(); // clear the TS flag of cr0

	fpu_handler();
}

/*
 * All of our Exception handling Interrupt Service Routines will
 * point to this function. This will tell us what exception has
 * occured! Right now, we simply abort the current task.
 * All ISRs disable interrupts while they are being
 * serviced as a 'locking' mechanism to prevent an IRQ from
 * happening and messing up kernel data structures
 */
static void arch_fault_handler(struct state *s)
{

	if (s->int_no < 32)
		LOG_INFO("%s", exception_messages[s->int_no]);
	else
		LOG_WARNING("Unknown exception %d", s->int_no);

	LOG_ERROR(" Exception (%d) on core %d at %#x:%#lx, fs = %#lx, gs = %#lx, error code = %#lx, task id = %u, rflags = %#x\n",
		s->int_no, CORE_ID, s->cs, s->rip, s->fs, s->gs, s->error, per_core(current_task)->id, s->rflags);
	LOG_ERROR("rax %#lx, rbx %#lx, rcx %#lx, rdx %#lx, rbp, %#lx, rsp %#lx rdi %#lx, rsi %#lx, r8 %#lx, r9 %#lx, r10 %#lx, r11 %#lx, r12 %#lx, r13 %#lx, r14 %#lx, r15 %#lx\n",
		s->rax, s->rbx, s->rcx, s->rdx, s->rbp, s->rsp, s->rdi, s->rsi, s->r8, s->r9, s->r10, s->r11, s->r12, s->r13, s->r14, s->r15);

	apic_eoi(s->int_no);
	//do_abort();
	sys_exit(-EFAULT);
}
