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

#ifdef __KERNEL__
#include <hermit/stddef.h>
#else
#include <stdlib.h>
#include <stdint.h>
#include <sys/types.h>

#ifndef NORETURN
#define NORETURN	__attribute__((noreturn))
#endif

typedef unsigned int tid_t;
#endif

#ifdef __cplusplus
extern "C" {
#endif

struct sem;
typedef struct sem sem_t;

typedef void (*signal_handler_t)(int);

/*
 * HermitCore is a libOS.
 * => classical system calls are realized as normal function
 * => forward declaration of system calls as function
 */
tid_t sys_getpid(void);
int sys_fork(void);
int sys_wait(int* status);
int sys_execve(const char* name, char * const * argv, char * const * env);
int sys_getprio(tid_t* id);
int sys_setprio(tid_t* id, int prio);
void NORETURN sys_exit(int arg);
ssize_t sys_read(int fd, char* buf, size_t len);
ssize_t sys_write(int fd, const char* buf, size_t len);
ssize_t sys_sbrk(ssize_t incr);
int sys_open(const char* name, int flags, int mode);
int sys_close(int fd);
void sys_msleep(unsigned int ms);
int sys_sem_init(sem_t** sem, unsigned int value);
int sys_sem_destroy(sem_t* sem);
int sys_sem_wait(sem_t* sem);
int sys_sem_post(sem_t* sem);
int sys_sem_timedwait(sem_t *sem, unsigned int ms);
int sys_sem_cancelablewait(sem_t* sem, unsigned int ms);
int sys_clone(tid_t* id, void* ep, void* argv);
off_t sys_lseek(int fd, off_t offset, int whence);
size_t sys_get_ticks(void);
int sys_rcce_init(int session_id);
size_t sys_rcce_malloc(int session_id, int ue);
int sys_rcce_fini(int session_id);
void sys_yield(void);
int sys_kill(tid_t dest, int signum);
int sys_signal(signal_handler_t handler);

struct ucontext;
typedef struct ucontext ucontext_t;

void makecontext(ucontext_t *ucp, void (*func)(), int argc, ...);
int swapcontext(ucontext_t *oucp, const ucontext_t *ucp);
int getcontext(ucontext_t *ucp);
int setcontext(ucontext_t *ucp);

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
#define __NR_stat		15
#define __NR_dup		16
#define __NR_dup2		17
#define __NR_msleep		18
#define __NR_yield		19
#define __NR_sem_init		20
#define __NR_sem_destroy	21
#define __NR_sem_wait		22
#define __NR_sem_post		23
#define __NR_sem_timedwait	24
#define __NR_getprio		25
#define __NR_setprio		26
#define __NR_clone		27
#define __NR_sem_cancelablewait	28
#define __NR_get_ticks		29

#ifndef __KERNEL__
inline static long
syscall(int nr, unsigned long arg0, unsigned long arg1, unsigned long arg2)
{
	long res;

	// note: syscall stores the return address in rcx and rflags in r11
	asm volatile ("syscall"
		: "=a" (res)
		: "a" (nr), "D" (arg0), "S" (arg1), "d" (arg2)
		: "memory", "%rcx", "%r11");

	return res;
}

#define SYSCALL0(NR) \
	syscall(NR, 0, 0, 0)
#define SYSCALL1(NR, ARG0) \
	syscall(NR, (unsigned long)ARG0, 0, 0)
#define SYSCALL2(NR, ARG0, ARG1) \
	syscall(NR, (unsigned long)ARG0, (unsigned long)ARG1, 0)
#define SYSCALL3(NR, ARG0, ARG1, ARG2) \
	syscall(NR, (unsigned long)ARG0, (unsigned long)ARG1, (unsigned long)ARG2)
#endif // __KERNEL__

#ifdef __cplusplus
}
#endif

#endif
