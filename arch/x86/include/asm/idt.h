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
 * @file arch/x86/include/asm/idt.h
 * @brief Definition of IDT flags and functions to set interrupts
 *
 * This file contains define-constants for interrupt flags
 * and installer functions for interrupt gates.\n
 * See idt.c for structure definitions.
 */

#ifndef __ARCH_IDT_H__
#define __ARCH_IDT_H__

#include <hermit/stddef.h>

/// This bit shall be set to 0 if the IDT slot is empty
#define IDT_FLAG_PRESENT 	0x80
/// Interrupt can be called from within RING0
#define IDT_FLAG_RING0		0x00
/// Interrupt can be called from within RING1 and lower
#define IDT_FLAG_RING1		0x20
/// Interrupt can be called from within RING2 and lower
#define IDT_FLAG_RING2		0x40
/// Interrupt can be called from within RING3 and lower
#define IDT_FLAG_RING3		0x60
/// Size of gate is 16 bit
#define IDT_FLAG_16BIT		0x00
/// Size of gate is 32 bit
#define IDT_FLAG_32BIT		0x08
/// The entry describes an interrupt gate
#define IDT_FLAG_INTTRAP	0x06
/// The entry describes a trap gate
#define IDT_FLAG_TRAPGATE	0x07
/// The entry describes a task gate
#define IDT_FLAG_TASKGATE	0x05

/* 
 * This is not IDT-flag related. It's the segment selectors for kernel code and data.
 */
#define KERNEL_CODE_SELECTOR	0x08
#define KERNEL_DATA_SELECTOR	0x10

#ifdef __cplusplus
extern "C" {
#endif

/** @brief Defines an IDT entry
 *
 * This structure defines interrupt descriptor table entries.\n
 * They consist of the handling function's base address, some flags 
 * and a segment selector.
 */
typedef struct {
	/// Handler function's lower 16 address bits
	uint16_t base_lo;
	/// Handler function's segment selector.
	uint16_t sel;
	/// index of the interrupt stack table
	uint8_t ist_index;
	/// These 8 bits contain flags. Exact use depends on the type of interrupt gate.
	uint8_t flags;
	/// Higher 16 bits of handler function's base address
	uint16_t base_hi;
	/// In 64 bit mode, the "highest" 32 bits of the handler function's base address
	uint32_t base_hi64;
	/// resvered entries
	uint32_t reserved;
} __attribute__ ((packed)) idt_entry_t;

/** @brief Defines the idt pointer structure.
 *
 * This structure keeps information about 
 * base address and size of the interrupt descriptor table.
 */
typedef struct {
	/// Size of the IDT in bytes (not the number of entries!)
	uint16_t limit;
	/// Base address of the IDT
	size_t base;
} __attribute__ ((packed)) idt_ptr_t;

/** @brief Installs IDT
 *
 * The installation involves the following steps:
 * - Set up the IDT pointer
 * - Set up int 0x80 for syscalls
 * - process idt_load()
 */
void idt_install(void);

/** @brief Set an entry in the IDT
 *
 * @param num index in the IDT
 * @param base base-address of the handler function being installed
 * @param sel Segment the IDT will use
 * @param flags Flags this entry will have
 * @param idx Index of interrupt stack table
 */
void idt_set_gate(uint8_t num, size_t base, uint16_t sel, uint8_t flags, uint8_t idx);

#ifdef __cplusplus
}
#endif

#endif
