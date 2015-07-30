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
 * This file prepare atomic operations on int32 & int64_t variables
 * which will be used in locking-mechanisms.
 */

#ifndef __ARCH_ATOMIC_H__
#define __ARCH_ATOMIC_H__

#include <hermit/stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#if MAX_CORES > 1
#define LOCK "lock ; "
#else
#define LOCK ""
#endif

/** @brief Makro for initialization of atomic vars
 *
 * Whenever you use an atomic variable, init it with 
 * this macro first.\n
 * Example: atomic_int32_t myAtomicVar = ATOMIC_INIT(123);
 *
 * @param i The number value you want to init it with.
 */
#define ATOMIC_INIT(i)  { (i) }


#include <asm/atomic32.h>
#include <asm/atomic64.h>

#ifdef __cplusplus
}
#endif

#endif
