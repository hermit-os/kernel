/*
 * Copyright (c) 2011, Stefan Lankes, RWTH Aachen University
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
 * @file include/hermit/syscall.h
 * @brief System call number definitions
 *
 * This file contains define constants for every syscall's number.
 */

#ifndef __SYSCALL_H__
#define __SYSCALL_H__

#include <hermit/stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define __NR_exit 		0
#define __NR_write		1
#define __NR_open		2
#define __NR_close		3
#define __NR_read		4
#define __NR_lseek		5
#define __NR_unlink		6
#define __NR_getpid		7
#define __NR_kill		8
#define __NR_fstat		9
#define __NR_sbrk		10
#define __NR_fork		11
#define __NR_wait		12
#define __NR_execve		13
#define __NR_times		14
#define __NR_accept		15
#define __NR_bind		16
#define __NR_closesocket	17
#define __NR_connect		18
#define __NR_listen		19
#define __NR_recv		20
#define __NR_send		21
#define __NR_socket		22
#define __NR_getsockopt		23
#define __NR_setsockopt		24
#define __NR_gethostbyname	25
#define __NR_sendto		26
#define __NR_recvfrom		27
#define __NR_select		28
#define __NR_stat		29
#define __NR_dup		30
#define __NR_dup2		31
#define __NR_msleep		32
#define __NR_yield		33
#define __NR_sem_init		34
#define __NR_sem_destroy	35
#define __NR_sem_wait		36
#define __NR_sem_post		37
#define __NR_sem_timedwait	38
#define __NR_getprio		39
#define __NR_setprio		40
#define __NR_clone		41
#define __NR_sem_cancelablewait	42
#define __NR_get_ticks		43

#ifdef __cplusplus
}
#endif

#endif
