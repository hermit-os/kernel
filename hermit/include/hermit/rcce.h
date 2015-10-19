/*
 * Copyright 2015 Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * This software is available to you under a choice of one of two
 * licenses.  You may choose to be licensed under the terms of the GNU
 * General Public License (GPL) Version 2 (https://www.gnu.org/licenses/gpl-2.0.txt)
 * or the BSD license below:
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

#ifndef __RCCE_H__
#define __RCCE_H__

#include <hermit/stddef.h>
#include <hermit/islelock.h>

#define RCCE_MAXNP	8
#define RCCE_MPB_SIZE	(64*1024)

typedef struct tas {
	volatile unsigned char reg;
	unsigned char reserved[CACHE_LINE-1];
} tas_t  __attribute__ ((aligned (CACHE_LINE)));

typedef struct rcce_mpb {
	int id; // session id;
	volatile size_t mpb[MAX_ISLE];
} rcce_mpb_t;

#define MAX_RCCE_SESSIONS 	((PAGE_SIZE - CACHE_LINE*(RCCE_MAXNP+1)) / sizeof(rcce_mpb_t))

extern islelock_t* rcce_lock;
extern rcce_mpb_t* rcce_mpb;
extern uint64_t phy_rcce_internals;

#endif
