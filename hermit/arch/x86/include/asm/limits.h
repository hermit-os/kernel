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
 * author Stefan Lankes
 * @file arch/x86/include/asm/limits.h
 * @brief Define constants related to numerical value-ranges of variable types
 *
 * This file contains define constants for the numerical 
 * ranges of the most typical variable types.
 */

#ifndef __ARCH_LIMITS_H__
#define __ARCH_LIMITS_H__

#ifdef __cplusplus
extern "C" {
#endif

/** Number of bits in a char */
#define	CHAR_BIT	8		

/** Maximum value for a signed char */
#define	SCHAR_MAX	0x7f		
/** Minimum value for a signed char */
#define	SCHAR_MIN	(-0x7f - 1)	

/** Maximum value for an unsigned char */
#define	UCHAR_MAX	0xff		

/** Maximum value for an unsigned short */
#define	USHRT_MAX	0xffff		
/** Maximum value for a short */
#define	SHRT_MAX	0x7fff		
/** Minimum value for a short */
#define	SHRT_MIN	(-0x7fff - 1)	

/** Maximum value for an unsigned int */
#define	UINT_MAX	0xffffffffU	
/** Maximum value for an int */
#define	INT_MAX		0x7fffffff	
/** Minimum value for an int */
#define	INT_MIN	(-0x7fffffff - 1)	

/** Maximum value for an unsigned long */
#define	ULONG_MAX	0xffffffffUL	
/** Maximum value for a long */
#define	LONG_MAX	0x7fffffffL	
/** Minimum value for a long */
#define	LONG_MIN	(-0x7fffffffL - 1)	

/** Maximum value for an unsigned long long */
#define	ULLONG_MAX	0xffffffffffffffffULL
/** Maximum value for a long long */
#define	LLONG_MAX	0x7fffffffffffffffLL	
/** Minimum value for a long long */
#define	LLONG_MIN	(-0x7fffffffffffffffLL - 1)  

#ifdef __cplusplus
}
#endif

#endif
