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
 * @file arch/x86/include/asm/irqflags.h
 * @brief Functions related to IRQ configuration
 *
 * This file contains definitions of inline functions 
 * for enabling and disabling IRQ handling.
 */

#ifndef __ARCH_IRQFLAGS_H__
#define __ARCH_IRQFLAGS_H__

#ifdef __cplusplus
extern "C" {
#endif

/** @brief Disable IRQs
 *
 * This inline function just clears out the interrupt bit
 */
inline static void irq_disable(void) {
	asm volatile("cli" ::: "memory");
}

/** @brief Disable IRQs (nested)
 *
 * Disable IRQs when unsure if IRQs were enabled at all.\n
 * This function together with irq_nested_enable can be used
 * in situations when interrupts shouldn't be activated if they
 * were not activated before calling this function.
 *
 * @return The set of flags which have been set until now
 */
inline static uint8_t irq_nested_disable(void) {
	size_t flags;
	asm volatile("pushf; cli; pop %0": "=r"(flags) : : "memory");
	if (flags & (1 << 9))
		return 1;	
	return 0;
}

/** @brief Enable IRQs */
inline static void irq_enable(void) {
	asm volatile("sti" ::: "memory");
}

/** @brief Enable IRQs (nested)
 *
 * If called after calling irq_nested_disable, this function will
 * not activate IRQs if they were not active before.
 *
 * @param flags Flags to set. Could be the old ones you got from irq_nested_disable.
 */
inline static void irq_nested_enable(uint8_t flags) {
	if (flags)
		irq_enable();
}

/** @brief Determines, if the interrupt flags (IF) is ser
 *
 * @return
 * - 1 interrupt flag is set
 * - 0 interrupt flag is cleared
 */ 
inline static uint8_t is_irq_enabled(void)
{
	size_t flags;
	asm volatile("pushf; pop %0": "=r"(flags) : : "memory");
	if (flags & (1 << 9))
		return 1;
	return 0;
}

#ifdef __cplusplus
}
#endif

#endif
