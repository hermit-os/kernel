/* 
 * Written by the Chair for Operating Systems, RWTH Aachen University
 * 
 * NO Copyright (C) 2010-2011, Stefan Lankes
 * consider these trivial functions to be public domain.
 * 
 * These functions are distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 */

/**
 * @author Stefan Lankes
 * @file arch/x86/include/asm/string.h
 * @brief Functions related to memcpy and strings.
 *
 * This file deals with memcpy, memset, string functions and everything related to
 * continuous byte fields.
 */

#ifndef __ARCH_STRING_H__
#define __ARCH_STRING_H__

#include <hermit/stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#if HAVE_ARCH_MEMCPY
/** @brief Copy a byte range from source to dest
 *
 * @param dest Destination address
 * @param src Source address
 * @param count Range of the byte field in bytes
 */
inline static void *memcpy(void* dest, const void *src, size_t count)
{
	size_t i, j, k;

	if (BUILTIN_EXPECT(!dest || !src, 0))
		return dest;

	asm volatile (
		"cld; rep movsq\n\t"
		"movq %4, %%rcx\n\t"
		"andq $7, %%rcx\n\t"
		"rep movsb\n\t"
		: "=&c"(i), "=&D"(j), "=&S"(k)
		: "0"(count/8), "g"(count), "1"(dest), "2"(src) : "memory","cc");

	return dest;
}
#endif

#if HAVE_ARCH_MEMSET
/** @brief Repeated write of a value to a whole range of bytes
 *
 * @param dest Destination address
 * @param val Value to flood the range with
 * @param count Size of target range in bytes
 */
inline static void *memset(void* dest, int val, size_t count)
{
	size_t i, j;

	if (BUILTIN_EXPECT(!dest, 0))
		return dest;

	if (val) {
		asm volatile ("cld; rep stosb"
			: "=&c"(i), "=&D"(j)
			: "a"(val), "1"(dest), "0"(count) : "memory","cc");
	} else {
		asm volatile (
			"cld; rep stosq\n\t"
			"movq %5, %%rcx\n\t"
			"andq $7, %%rcx\n\t"
			"rep stosb\n\t"
			: "=&c"(i), "=&D"(j)
			: "a"(0x00ULL), "1"(dest), "0"(count/8), "g"(count): "memory","cc");
	}

	return dest;
}
#endif

#if HAVE_ARCH_STRLEN
/** @brief Standard string length
 *
 * This function computed the length of the given null terminated string
 * just like the strlen functions you are used to.
 *
 * @return 
 * - The length of the string
 * - 0 if str is a NULL pointer
 */
inline static size_t strlen(const char* str)
{
	size_t len = 0;
	size_t i, j;

	if (BUILTIN_EXPECT(!str, 0))
		return len;

	asm volatile("not %%rcx; cld; repne scasb; not %%rcx; dec %%rcx"
		: "=&c"(len), "=&D"(i), "=&a"(j)
		: "2"(0), "1"(str), "0"(len)
		: "memory","cc");

	return len;
}
#endif

#if HAVE_ARCH_STRNCPY
/** @brief Copy string with maximum of n byte length
 *
 * @param dest Destination string pointer
 * @param src Source string pointer
 * @param n maximum number of bytes to copy
 */
char* strncpy(char* dest, const char* src, size_t n);
#endif

#if HAVE_ARCH_STRCPY
/** @brief Copy string
 *
 * Note that there is another safer variant of this function: strncpy.\n
 * That one could save you from accidents with buffer overruns.
 *
 * @param dest Destination string pointer
 * @param src Source string pointer
 */
char* strcpy(char* dest, const char* src);
#endif

#ifdef __cplusplus
}
#endif

#endif
