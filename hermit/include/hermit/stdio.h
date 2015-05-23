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
 * @file include/hermit/stdio.h
 * @brief Stringstream related functions. Mainly printf-stuff.
 */

#ifndef __STDIO_H__
#define __STDIO_H__

#include <hermit/config.h>
#include <hermit/stddef.h>
#include <hermit/stdarg.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Works like the ANSI C function puts 
 */
int kputs(const char *);

/**
 * Works like the ANSI C function putchar
 */
int kputchar(int);

/**
 * Works like the ANSI C function printf
 */
int kprintf(const char*, ...);

/**
 * Initialize the I/O functions 
 */
int koutput_init(void);

/**
 * Works like the ANSI c function sprintf
 */
int ksprintf(char *str, const char *format, ...);

/**
 * Works like the ANSI c function sprintf
 */
int ksnprintf(char *str, size_t size, const char *format, ...);

/**
 * Scaled down version of printf(3)
 */
int kvprintf(char const *fmt, void (*func) (int, void *), void *arg, int radix, va_list ap);

#ifdef __cplusplus
}
#endif

#endif
