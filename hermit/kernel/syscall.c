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

static tid_t sys_getpid(void)
{
	task_t* task = per_core(current_task);

	return task->id;
}

static int sys_getprio(void)
{
	task_t* task = per_core(current_task);

	return task->prio;
}

static void sys_yield(void)
{
	reschedule();
}

static int sys_write(int fd, const char* buf, size_t len)
{
	size_t i;

	//TODO: Currently, we ignore the file descriptor

	if (BUILTIN_EXPECT(!buf, 0))
		return -1;

	for(i=0; i<len; i++)
		kputchar(buf[i]);

	return 0;
}

static ssize_t sys_sbrk(int incr)
{
	task_t* task = per_core(current_task);
	vma_t* heap = task->heap;
	ssize_t ret;

	spinlock_lock(&task->vma_lock);

	if (BUILTIN_EXPECT(!heap, 0)) {
		kprintf("sys_sbrk: missing heap!\n");
		abort();
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

static int sys_open(const char* name, int flags, int mode)
{
	return 0;
}

static int sys_close(int fd)
{
	return 0;
}

static int sys_msleep(unsigned int msec)
{
	timer_wait(msec*TIMER_FREQ/1000);

	return 0;
}

static int sys_sem_init(sem_t** sem, unsigned int value)
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

static int sys_sem_destroy(sem_t* sem)
{
	int ret;

	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	ret = sem_destroy(sem);
	if (!ret)
		kfree(sem);

	return ret;
}

static int sys_sem_wait(sem_t* sem)
{
	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	return sem_wait(sem, 0);
}

static int sys_sem_post(sem_t* sem)
{
	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	return sem_post(sem);
}

static int sys_sem_timedwait(sem_t *sem, unsigned int ms)
{
	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	return sem_wait(sem, ms);
}

static int sys_clone(tid_t* id, void* ep, void* argv)
{
	return clone_task(id, ep, argv, per_core(current_task)->prio, CORE_ID);	
}

static int default_handler(void)
{
	kprintf("Invalid system call\n");

	return -ENOSYS;
}

size_t syscall_table[] = {
	(size_t) sys_exit,		/* __NR_exit 	*/
	(size_t) sys_write,		/* __NR_write 	*/
	(size_t) sys_open, 		/* __NR_open 	*/
	(size_t) sys_close,		/* __NR_close 	*/
	(size_t) default_handler,	/* __NR_read 	*/
	(size_t) default_handler,	/* __NR_lseek	*/
	(size_t) default_handler, 	/* __NR_unlink	*/
	(size_t) sys_getpid, 		/* __NR_getpid	*/
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
	(size_t) default_handler, 	/* __NR_gethostbyname	*/
	(size_t) default_handler,	/* __NR_sendto	*/
	(size_t) default_handler,	/* __NR_recvfrom	*/
	(size_t) default_handler,	/* __NR_select	*/
	(size_t) default_handler,	/* __NR_stat	*/
	(size_t) default_handler,	/* __NR_dup	*/
	(size_t) default_handler,	/* __NR_dup2	*/
	(size_t) sys_msleep,		/* __NR_msleep	*/
	(size_t) sys_yield,		/* __NR_yield	*/
	(size_t) sys_sem_init,	 	/* __NR_sem_init	*/
	(size_t) sys_sem_destroy, 	/* __NR_sem_destroy	*/
	(size_t) sys_sem_wait,	 	/* __NR_sem_wait	*/
	(size_t) sys_sem_post,	 	/* __NR_sem_post	*/
	(size_t) sys_sem_timedwait, 	/* __NR_sem_timedwait	*/
	(size_t) sys_getprio,		/* __NR_getprio	*/
	(size_t) default_handler,	/* __NR_setprio	*/
	(size_t) sys_clone		/* __NR_clone	*/
};
