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
 * @file arch/x86/include/asm/io.h
 * @brief Functions related to processor IO
 *
 * This file contains inline functions for processor IO operations.
 */

#ifndef __ARCH_IO_H__
#define __ARCH_IO_H__

#ifdef __cplusplus
extern "C" {
#endif

/** @brief Read a byte from an IO port
 *
 * @param _port The port you want to read from
 * @return The value which reads out from this port
 */
inline static unsigned char inportb(unsigned short _port) {
	unsigned char rv;
	asm volatile("inb %1, %0":"=a"(rv):"dN"(_port));
	return rv;
} 

/** @brief Read a word (2 byte) from an IO port
 *
 * @param _port The port you want to read from
 * @return The value which reads out from this port
 */
inline static unsigned short inportw(unsigned short _port) {
	unsigned short rv;
	asm volatile("inw %1, %0":"=a"(rv):"dN"(_port));
	return rv;
}

/** @brief Read a double word (4 byte) from an IO port
 *
 * @param _port The port you want to read from
 * @return The value which reads out from this port
 */
inline static unsigned int inportl(unsigned short _port) {
	unsigned int rv;
	asm volatile("inl %1, %0":"=a"(rv):"dN"(_port));
	return rv;
}

/** @brief Write a byte to an IO port
 *
 * @param _port The port you want to write to
 * @param _data the 1 byte value you want to write
 */
inline static void outportb(unsigned short _port, unsigned char _data) {
	asm volatile("outb %1, %0"::"dN"(_port), "a"(_data));
}

/** @brief Write a word (2 bytes) to an IO port
 *
 * @param _port The port you want to write to
 * @param _data the 2 byte value you want to write
 */
inline static void outportw(unsigned short _port, unsigned short _data) {
	asm volatile("outw %1, %0"::"dN"(_port), "a"(_data));
}

/** @brief Write a double word (4 bytes) to an IO port
 *
 * @param _port The port you want to write to
 * @param _data the 4 byte value you want to write
 */
inline static void outportl(unsigned short _port, unsigned int _data)
{
	 asm volatile("outl %1, %0"::"dN"(_port), "a"(_data));
}

#ifdef __cplusplus
}
#endif

#endif
