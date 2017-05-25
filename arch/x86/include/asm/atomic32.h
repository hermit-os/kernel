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
 * @file arch/x86/include/asm/atomic.h
 * @brief Functions for atomic operations
 *
 * This file defines functions for atomic operations on int32 variables
 * which will be used in locking-mechanisms.
 */

#ifndef __ARCH_ATOMIC32_H__
#define __ARCH_ATOMIC32_H__

#ifdef __cplusplus
extern "C" {
#endif

/** @brief Standard-datatype for atomic operations
 *
 * It just consists of an int32_t variable internally, marked as volatile.
 */
typedef struct { volatile int32_t counter; } atomic_int32_t;

/** @brief Atomic test and set operation for int32 vars.
 *
 * This function will atomically exchange the value of an atomic variable and
 * return its old value. Is used in locking-operations.\n
 * \n
 * Intel manuals: If a memory operand is referenced, the processor's locking
 * protocol is automatically implemented for the duration of the exchange
 * operation, regardless of the presence or absence of the LOCK prefix.
 *
 * @param d Pointer to the atomic_int_32_t with the value you want to exchange
 * @param ret the value you want the var test for
 *
 * @return The old value of the atomic_int_32_t var before exchange
 */
inline static int32_t atomic_int32_test_and_set(atomic_int32_t* d, int32_t ret)
{
	asm volatile ("xchgl %0, %1" : "=r"(ret) : "m"(d->counter), "0"(ret) : "memory");
	return ret;
}

/** @brief Atomic addition of values to atomic_int32_t vars
 *
 * This function lets you add values in an atomic operation
 *
 * @param d Pointer to the atomit_int32_t var you want do add a value to
 * @param i The value you want to increment by
 *
 * @return The mathematical result
 */
inline static int32_t atomic_int32_add(atomic_int32_t *d, int32_t i)
{
	int32_t res = i;
	asm volatile(LOCK "xaddl %0, %1" : "+r"(i), "+m"(d->counter) : : "memory", "cc");
	return res+i;
}

/** @brief Atomic subtraction of values from atomic_int32_t vars
 *
 * This function lets you subtract values in an atomic operation.\n
 * This function is just for convenience. It uses atomic_int32_add(d, -i)
 *
 * @param d Pointer to the atomic_int32_t var you want to subtract from
 * @param i The value you want to subtract by
 *
 * @return The mathematical result
 */
inline static int32_t atomic_int32_sub(atomic_int32_t *d, int32_t i)
{
    return atomic_int32_add(d, -i);
}

/** @brief Atomic increment by one
 *
 * The atomic_int32_t var will be atomically incremented by one.\n
 *
 * @param d The atomic_int32_t var you want to increment
 */
inline static int32_t atomic_int32_inc(atomic_int32_t* d) {
	int32_t res = 1;
	asm volatile(LOCK "xaddl %0, %1" : "+r"(res), "+m"(d->counter) : : "memory", "cc");
	return ++res;
}

/** @brief Atomic decrement by one
 *
 * The atomic_int32_t var will be atomically decremented by one.\n
 *
 * @param d The atomic_int32_t var you want to decrement
 */
inline static int32_t atomic_int32_dec(atomic_int32_t* d) {
	int32_t res = -1;
	asm volatile(LOCK "xaddl %0, %1" : "+r"(res), "+m"(d->counter) : : "memory", "cc");
	return --res;
}

/** @brief Read out an atomic_int32_t var
 *
 * This function is for convenience: It looks into the atomic_int32_t struct
 * and returns the internal value for you.
 *
 * @param d Pointer to the atomic_int32_t var you want to read out
 * @return It's number value
 */
inline static int32_t atomic_int32_read(atomic_int32_t *d) {
	return d->counter;
}

/** @brief Set the value of an atomic_int32_t var
 *
 * This function is for convenience: It sets the internal value of
 * an atomic_int32_t var for you.
 *
 * @param d Pointer to the atomic_int32_t var you want to set
 * @param v The value to set
 */
inline static void atomic_int32_set(atomic_int32_t *d, int32_t v) {
	atomic_int32_test_and_set(d, v);
}

#ifdef __cplusplus
}
#endif

#endif
