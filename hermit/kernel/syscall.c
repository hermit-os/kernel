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

#include <hermit/stddef.h>
#include <hermit/stdio.h>
#include <hermit/tasks.h>
#include <hermit/errno.h>
#include <hermit/syscall.h>
#include <hermit/spinlock.h>
#include <hermit/semaphore.h>
#include <hermit/time.h>
#include <hermit/rcce.h>
#include <hermit/memory.h>

#include <lwip/sockets.h>
#include <lwip/err.h>
#include <lwip/stats.h>

//TODO: don't use one big kernel lock to comminicate with all proxies
static spinlock_t lwip_lock = SPINLOCK_INIT;

extern int32_t isle;
extern int32_t possible_isles;
extern int libc_sd;

tid_t sys_getpid(void)
{
	task_t* task = per_core(current_task);

	return task->id;
}

int sys_getprio(void)
{
	task_t* task = per_core(current_task);

	return task->prio;
}

int sys_setprio(tid_t* id, int prio)
{
	return -ENOSYS;
}

static void sys_yield(void)
{
	reschedule();
}

void NORETURN do_exit(int arg);

typedef struct {
	int sysnr;
	int arg;
} __attribute__((packed)) sys_exit_t;

/** @brief To be called by the systemcall to exit tasks */
void NORETURN sys_exit(int arg)
{
	sys_exit_t sysargs = {__NR_exit, arg};

	if (libc_sd >= 0)
	{
		spinlock_lock(&lwip_lock);
		write(libc_sd, &sysargs, sizeof(sysargs));
		spinlock_unlock(&lwip_lock);

		closesocket(libc_sd);
		libc_sd = -1;
	}

	do_exit(arg);
}

typedef struct {
	int sysnr;
	int fd;
	size_t len;
} __attribute__((packed)) sys_read_t;

ssize_t sys_read(int fd, char* buf, size_t len)
{
	sys_read_t sysargs = {__NR_read, fd, len};
	ssize_t j, ret;

	if (libc_sd < 0)
		return -ENOSYS;

	spinlock_lock(&lwip_lock);
	write(libc_sd, &sysargs, sizeof(sysargs));

	read(libc_sd, &j, sizeof(j));
	if (j > 0)
	{
		ssize_t i = 0;

		while(i < j)
		{
			ret = read(libc_sd, buf+i, j-i);
			if (ret < 0) {
				spinlock_unlock(&lwip_lock);
				return ret;
			}

			i += ret;
		}
	}

	spinlock_unlock(&lwip_lock);

	return j;
}

typedef struct {
	int sysnr;
	int fd;
	size_t len;
} __attribute__((packed)) sys_write_t;

ssize_t sys_write(int fd, const char* buf, size_t len)
{
	ssize_t i, ret;
	int flag;
	sys_write_t sysargs = {__NR_write, fd, len};

	if (BUILTIN_EXPECT(!buf, 0))
		return -1;

	if (libc_sd < 0)
	{
		for(i=0; i<len; i++)
			kputchar(buf[i]);

		return len;
	}

	spinlock_lock(&lwip_lock);

	flag = 0;
	setsockopt(libc_sd, IPPROTO_TCP, TCP_NODELAY, (char *) &flag, sizeof(flag));

	write(libc_sd, &sysargs, sizeof(sysargs));

	i=0;
	while(i < len)
	{
		ret = write(libc_sd, (char*)buf+i, len-i);
		if (ret < 0) {
			spinlock_unlock(&lwip_lock);
			return ret;
		}

		i += ret;
	}

	flag = 1;
	setsockopt(libc_sd, IPPROTO_TCP, TCP_NODELAY, (char *) &flag, sizeof(flag));

	if (fd > 2) {
		ret = read(libc_sd, &i, sizeof(i));
		if (ret < 0)
			i = ret;
	} else i = len;

	spinlock_unlock(&lwip_lock);

	return i;
}

ssize_t sys_sbrk(int incr)
{
	task_t* task = per_core(current_task);
	vma_t* heap = task->heap;
	ssize_t ret;

	spinlock_lock(&task->vma_lock);

	if (BUILTIN_EXPECT(!heap, 0)) {
		kprintf("sys_sbrk: missing heap!\n");
		do_abort();
	}

	ret = heap->end;
	heap->end += incr;
	if (heap->end < heap->start)
		heap->end = heap->start;

	// allocation and mapping of new pages for the heap
	// is catched by the pagefault handler

	spinlock_unlock(&task->vma_lock);

	return ret;
}

int sys_open(const char* name, int flags, int mode)
{
	int i, ret, sysnr = __NR_open;
	size_t len;

	if (libc_sd < 0)
		return 0;

	len = strlen(name)+1;

	spinlock_lock(&lwip_lock);

	i = 0;
	setsockopt(libc_sd, IPPROTO_TCP, TCP_NODELAY, (char *) &i, sizeof(i));

	ret = write(libc_sd, &sysnr, sizeof(sysnr));
	if (ret < 0)
		goto out;

	ret = write(libc_sd, &len, sizeof(len));
	if (ret < 0)
		goto out;

	i=0;
	while(i<len)
	{
		ret = write(libc_sd, name+i, len-i);
		if (ret < 0)
			goto out;
		i += ret;
	}

	ret = write(libc_sd, &flags, sizeof(flags));
	if (ret < 0)
		goto out;

	ret = write(libc_sd, &mode, sizeof(mode));
	if (ret < 0)
		goto out;

	i = 1;
	setsockopt(libc_sd, IPPROTO_TCP, TCP_NODELAY, (char *) &i, sizeof(i));

	read(libc_sd, &ret, sizeof(ret));

out:
	spinlock_unlock(&lwip_lock);

	return ret;
}

typedef struct {
	int sysnr;
	int fd;
} __attribute__((packed)) sys_close_t;

int sys_close(int fd)
{
	int ret;
	sys_close_t sysargs = {__NR_close, fd};

	if (libc_sd < 0)
		return 0;

	spinlock_lock(&lwip_lock);

	ret = write(libc_sd, &sysargs, sizeof(sysargs));
	if (ret != sizeof(sysargs))
		goto out;
	read(libc_sd, &ret, sizeof(ret));

out:
	spinlock_unlock(&lwip_lock);

	return ret;
}

int sys_msleep(unsigned int ms)
{
	if (ms * TIMER_FREQ / 1000 > 0)
		timer_wait(ms * TIMER_FREQ / 1000);
	else if (ms > 0)
		udelay(ms * 1000);

	return 0;
}

int sys_sem_init(sem_t** sem, unsigned int value)
{
	int ret;

	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	*sem = (sem_t*) kmalloc(sizeof(sem_t));
	if (BUILTIN_EXPECT(!(*sem), 0))
		return -ENOMEM;

	ret = sem_init(*sem, value);
	if (ret) {
		kfree(*sem);
		*sem = NULL;
	}

	return ret;
}

int sys_sem_destroy(sem_t* sem)
{
	int ret;

	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	ret = sem_destroy(sem);
	if (!ret)
		kfree(sem);

	return ret;
}

int sys_sem_wait(sem_t* sem)
{
	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	return sem_wait(sem, 0);
}

int sys_sem_post(sem_t* sem)
{
	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	return sem_post(sem);
}

int sys_sem_timedwait(sem_t *sem, unsigned int ms)
{
	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	return sem_wait(sem, ms);
}

int sys_sem_cancelablewait(sem_t* sem, unsigned int ms)
{
	return -ENOSYS;
}

int sys_clone(tid_t* id, void* ep, void* argv)
{
	return clone_task(id, ep, argv, per_core(current_task)->prio);
}

typedef struct {
	int sysnr;
	int fd;
	off_t offset;
	int whence;
} __attribute__((packed)) sys_lseek_t;

off_t sys_lseek(int fd, off_t offset, int whence)
{
	off_t off;
	sys_lseek_t sysargs = {__NR_lseek, fd, offset, whence};

	if (libc_sd < 0)
		return -ENOSYS;

	spinlock_lock(&lwip_lock);

	write(libc_sd, &sysargs, sizeof(sysargs));
	read(libc_sd, &off, sizeof(off));

	spinlock_unlock(&lwip_lock);

	return off;
}

static int sys_rcce_init(int session_id)
{
	int i, err = 0;
	size_t paddr = 0;

	if (session_id <= 0)
		return -EINVAL;

	islelock_lock(rcce_lock);

	for(i=0; i<MAX_RCCE_SESSIONS; i++)
	{
		if (rcce_mpb[i].id == session_id)
			break;
	}

	// create new session
	if (i >=MAX_RCCE_SESSIONS)
	{
		for(i=0; i<MAX_RCCE_SESSIONS; i++)
		{
			if (rcce_mpb[i].id == 0) {
				rcce_mpb[i].id = session_id;
				break;
			}
		}
	}

	if (i >= MAX_RCCE_SESSIONS)
	{
		err = -EINVAL;
		goto out;
	}

	paddr = get_pages(RCCE_MPB_SIZE / PAGE_SIZE);
	if (BUILTIN_EXPECT(!paddr, 0))
	{
		err = -ENOMEM;
		goto out;
	}

	rcce_mpb[i].mpb[isle] = paddr;

out:
	islelock_unlock(rcce_lock);

	kprintf("Create MPB for session %d at 0x%zx, using of slot %d\n", session_id, paddr, i);

	return err;
}

static size_t sys_rcce_malloc(int session_id, int ue)
{
	size_t vaddr = 0;
	int i, counter = 0;

	if (session_id <= 0)
		return -EINVAL;

	// after 120 retries (= 120*300 ms) we give up
	do {
		for(i=0; i<MAX_RCCE_SESSIONS; i++)
		{
			if ((rcce_mpb[i].id == session_id) && rcce_mpb[i].mpb[ue])
				break;
		}

		if (i >= MAX_RCCE_SESSIONS) {
			counter++;
			timer_wait((300*TIMER_FREQ)/1000);
		}
	} while((i >= MAX_RCCE_SESSIONS) && (counter < 120));

	//kprintf("i = %d, counter = %d, max %d\n", i, counter, MAX_RCCE_SESSIONS);

	// create new session
	if (i >= MAX_RCCE_SESSIONS)
		goto out;

	vaddr = vma_alloc(RCCE_MPB_SIZE, VMA_READ|VMA_WRITE|VMA_USER|VMA_CACHEABLE);
        if (BUILTIN_EXPECT(!vaddr, 0))
		goto out;

	if (page_map(vaddr, rcce_mpb[i].mpb[ue], RCCE_MPB_SIZE / PAGE_SIZE, PG_RW|PG_USER|PG_PRESENT)) {
		vma_free(vaddr, vaddr + 2*PAGE_SIZE);
		goto out;
	}

	kprintf("Map MPB of session %d at 0x%zx, using of slot %d, isle %d\n", session_id, vaddr, i, ue);

	return vaddr;

out:
	kprintf("Didn't find a valid MPB for session %d, isle %d\n", session_id, ue);

	return 0;
}

static int sys_rcce_fini(int session_id)
{
	int i, j;
	int ret = 0;

	// we have to free the MPB

	if (session_id <= 0)
		return -EINVAL;

	islelock_lock(rcce_lock);

	for(i=0; i<MAX_RCCE_SESSIONS; i++)
	{
		if (rcce_mpb[i].id == session_id)
			break;
	}

	if (i >= MAX_RCCE_SESSIONS) {
		ret = -EINVAL;
		goto out;
	}

	if (rcce_mpb[i].mpb[isle])
		put_pages(rcce_mpb[i].mpb[isle], RCCE_MPB_SIZE / PAGE_SIZE);
	rcce_mpb[i].mpb[isle] = 0;

	for(j=0; (j<MAX_ISLE) && !rcce_mpb[i].mpb[j]; j++)
		;

	// rest full session
	if (j >= MAX_ISLE)
		rcce_mpb[i].id = 0;

out:
	islelock_unlock(rcce_lock);

	return ret;
}

size_t sys_get_ticks(void)
{
	return get_clock_tick();
}

int sys_stat(const char* file, /*struct stat *st*/ void* st)
{
	return -ENOSYS;
}

static int default_handler(void)
{
#if 0
	kprintf("Invalid system call\n");
#else
	uint64_t rax;

	asm volatile ("mov %%rax, %0" : "=m"(rax) :: "memory");
	kprintf("Invalid system call: %zd\n", rax);
#endif
	return -ENOSYS;
}

size_t syscall_table[] = {
	(size_t) sys_exit,		/* __NR_exit	*/
	(size_t) sys_write,		/* __NR_write	*/
	(size_t) sys_open,		/* __NR_open	*/
	(size_t) sys_close,		/* __NR_close	*/
	(size_t) sys_read,		/* __NR_read	*/
	(size_t) sys_lseek,		/* __NR_lseek	*/
	(size_t) default_handler,	/* __NR_unlink	*/
	(size_t) sys_getpid,		/* __NR_getpid	*/
	(size_t) default_handler,	/* __NR_kill	*/
	(size_t) default_handler,	/* __NR_fstat	*/
	(size_t) sys_sbrk,		/* __NR_sbrk	*/
	(size_t) default_handler,	/* __NR_fork	*/
	(size_t) default_handler,	/* __NR_wait	*/
	(size_t) default_handler,	/* __NR_execve	*/
	(size_t) default_handler,	/* __NR_times	*/
	(size_t) default_handler,	/* __NR_accept	*/
	(size_t) default_handler,	/* __NR_bind	*/
	(size_t) default_handler,	/* __NR_closesocket	*/
	(size_t) default_handler,	/* __NR_connect	*/
	(size_t) default_handler,	/* __NR_listen	*/
	(size_t) default_handler,	/* __NR_recv	*/
	(size_t) default_handler,	/* __NR_send	*/
	(size_t) default_handler,	/* __NR_socket	*/
	(size_t) default_handler,	/* __NR_getsockopt	*/
	(size_t) default_handler,	/* __NR_setsockopt	*/
	(size_t) default_handler,	/* __NR_gethostbyname	*/
	(size_t) default_handler,	/* __NR_sendto	*/
	(size_t) default_handler,	/* __NR_recvfrom	*/
	(size_t) default_handler,	/* __NR_select	*/
	(size_t) default_handler,	/* __NR_stat	*/
	(size_t) default_handler,	/* __NR_dup	*/
	(size_t) default_handler,	/* __NR_dup2	*/
	(size_t) sys_msleep,		/* __NR_msleep	*/
	(size_t) sys_yield,		/* __NR_yield	*/
	(size_t) sys_sem_init,		/* __NR_sem_init	*/
	(size_t) sys_sem_destroy,	/* __NR_sem_destroy	*/
	(size_t) sys_sem_wait,		/* __NR_sem_wait	*/
	(size_t) sys_sem_post,		/* __NR_sem_post	*/
	(size_t) sys_sem_timedwait,	/* __NR_sem_timedwait	*/
	(size_t) sys_getprio,		/* __NR_getprio	*/
	(size_t) default_handler,	/* __NR_setprio	*/
	(size_t) sys_clone,		/* __NR_clone	*/
	(size_t) sys_sem_timedwait,	/* __NR_sem_cancelablewait	*/
	(size_t) sys_get_ticks,		/* __NR_get_ticks	*/
	(size_t) sys_rcce_init,		/* __NR_rcce_init	*/
	(size_t) sys_rcce_fini,		/* __NR_rcce_fini	*/
	(size_t) sys_rcce_malloc	/* __NR_rcce_malloc	*/
};
