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
 * @file arch/x86/include/asm/irq.h
 * @brief Functions related to IRQs
 *
 * This file contains functions and a pointer type related to interrupt requests.
 */

#ifndef __ARCH_IRQ_H__
#define __ARCH_IRQ_H__

#include <hermit/stddef.h>
//#include <hermit/tasks_types.h>

#ifdef __cplusplus
extern "C" {
#endif

/** @brief Pointer-type to IRQ-handling functions
 *
 * Whenever you write a IRQ-handling function it has to match this signature.
 */
typedef void (*irq_handler_t)(struct state *);

/** @brief Install a custom IRQ handler for a given IRQ 
 *
 * @param irq The desired irq
 * @param handler The handler to install
 */
int irq_install_handler(unsigned int irq, irq_handler_t handler);

/** @brief Clear the handler for a given IRQ 
 *
 * @param irq The handler's IRQ
 */
int irq_uninstall_handler(unsigned int irq);

/** @brief Procedure to initialize IRQ
 *
 * This procedure is just a small collection of calls:
 * - idt_install();
 * - isrs_install();
 * - irq_install();
 *
 * @return Just returns 0 in any case
 */
int irq_init(void);

/** @brief reset the counters of the received interrupts
 */
void reset_irq_stats(void);

/** @brief Print the number of received interrupts
 */
void print_irq_stats(void);

/** @brief Switch from a fix to a dynamic timer period
 *
 * @return 0 on success
 */
int enable_dynticks(void);

#ifdef __cplusplus
}
#endif

#endif
