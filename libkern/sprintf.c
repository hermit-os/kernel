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

typedef struct {
	char *str;
	size_t pos;
	size_t max;
} sputchar_arg_t;

static void sputchar(int c, void *arg)
{
	sputchar_arg_t *dest = (sputchar_arg_t *) arg;

	if (dest->pos < dest->max) {
		dest->str[dest->pos] = (char)c;
		dest->pos++;
	}
}

int ksnprintf(char *str, size_t size, const char *format, ...)
{
	int ret;
	va_list ap;
	sputchar_arg_t dest;

	dest.str = str;
	dest.pos = 0;
	dest.max = size;

	va_start(ap, format);
	ret = kvprintf(format, sputchar, &dest, 10, ap);
	va_end(ap);

	str[ret] = 0;

	return ret;
}

int ksprintf(char *str, const char *format, ...)
{
	int ret;
	va_list ap;
	sputchar_arg_t dest;

	dest.str = str;
	dest.pos = 0;
	dest.max = (size_t) -1;

	va_start(ap, format);
	ret = kvprintf(format, sputchar, &dest, 10, ap);
	va_end(ap);

	str[ret] = 0;

	return ret;
}
