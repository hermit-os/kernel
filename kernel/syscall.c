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
#include <hermit/signal.h>
#include <hermit/logging.h>
#include <asm/uhyve.h>
#include <sys/poll.h>

#include <lwip/sockets.h>
#include <lwip/err.h>
#include <lwip/stats.h>

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void kernel_start;

//TODO: don't use one big kernel lock to comminicate with all proxies
static spinlock_irqsave_t lwip_lock = SPINLOCK_IRQSAVE_INIT;

extern spinlock_irqsave_t stdio_lock;
extern int32_t isle;
extern int32_t possible_isles;
extern volatile int libc_sd;

tid_t sys_getpid(void)
{
	task_t* task = per_core(current_task);

	return task->id;
}

int sys_getprio(tid_t* id)
{
	task_t* task = per_core(current_task);

	if (!id || (task->id == *id))
		return task->prio;
	return -EINVAL;
}

int sys_setprio(tid_t* id, int prio)
{
	return -ENOSYS;
}

void NORETURN do_exit(int arg);

typedef struct {
	int sysnr;
	int arg;
} __attribute__((packed)) sys_exit_t;

/** @brief To be called by the systemcall to exit tasks */
void NORETURN sys_exit(int arg)
{
	if (is_uhyve()) {
		uhyve_send(UHYVE_PORT_EXIT, (unsigned) virt_to_phys((size_t) &arg));
	} else {
		sys_exit_t sysargs = {__NR_exit, arg};

		spinlock_irqsave_lock(&lwip_lock);
		if (libc_sd >= 0)
		{
			int s = libc_sd;

			lwip_write(s, &sysargs, sizeof(sysargs));
			libc_sd = -1;

			spinlock_irqsave_unlock(&lwip_lock);

			// switch to LwIP thread
			reschedule();

			lwip_close(s);
		} else {
			spinlock_irqsave_unlock(&lwip_lock);
		}
	}

	do_exit(arg);
}

typedef struct {
	int sysnr;
	int fd;
	size_t len;
} __attribute__((packed)) sys_read_t;

typedef struct {
	int fd;
	char* buf;
        size_t len;
	ssize_t ret;
} __attribute__((packed)) uhyve_read_t;

ssize_t sys_read(int fd, char* buf, size_t len)
{
	if (is_uhyve()) {
                uhyve_read_t uhyve_args = {fd, (char*) virt_to_phys((size_t) buf), len, -1};

                uhyve_send(UHYVE_PORT_READ, (unsigned)virt_to_phys((size_t)&uhyve_args));

                return uhyve_args.ret;
        }

	sys_read_t sysargs = {__NR_read, fd, len};
	ssize_t j, ret;
	int s;

	// do we have an LwIP file descriptor?
	if (fd & LWIP_FD_BIT) {
		ret = lwip_read(fd & ~LWIP_FD_BIT, buf, len);
		if (ret < 0)
			return -errno;

		return ret;
	}

	spinlock_irqsave_lock(&lwip_lock);
	if (libc_sd < 0) {
		spinlock_irqsave_unlock(&lwip_lock);
		return -ENOSYS;
	}

	s = libc_sd;
	lwip_write(s, &sysargs, sizeof(sysargs));

	lwip_read(s, &j, sizeof(j));
	if (j > 0)
	{
		ssize_t i = 0;

		while(i < j)
		{
			ret = lwip_read(s, buf+i, j-i);
			if (ret < 0) {
				spinlock_irqsave_unlock(&lwip_lock);
				return ret;
			}

			i += ret;
		}
	}

	spinlock_irqsave_unlock(&lwip_lock);

	return j;
}

ssize_t readv(int d, const struct iovec *iov, int iovcnt)
{
	return -ENOSYS;
}

typedef struct {
	int sysnr;
	int fd;
	size_t len;
} __attribute__((packed)) sys_write_t;

typedef struct {
	int fd;
	const char* buf;
	size_t len;
} __attribute__((packed)) uhyve_write_t;

ssize_t sys_write(int fd, const char* buf, size_t len)
{
	if (BUILTIN_EXPECT(!buf, 0))
		return -1;

	if (is_uhyve()) {
		uhyve_write_t uhyve_args = {fd, (const char*) virt_to_phys((size_t) buf), len};

		uhyve_send(UHYVE_PORT_WRITE, (unsigned)virt_to_phys((size_t)&uhyve_args));

		return uhyve_args.len;
	}

	ssize_t i, ret;
	int s;
	sys_write_t sysargs = {__NR_write, fd, len};

	// do we have an LwIP file descriptor?
	if (fd & LWIP_FD_BIT) {
		ret = lwip_write(fd & ~LWIP_FD_BIT, buf, len);
		if (ret < 0)
			return -errno;

		return ret;
	}

	spinlock_irqsave_lock(&lwip_lock);
	if (libc_sd < 0)
	{
		spinlock_irqsave_unlock(&lwip_lock);

		spinlock_irqsave_lock(&stdio_lock);
		for(i=0; i<len; i++)
			kputchar(buf[i]);
		spinlock_irqsave_unlock(&stdio_lock);

		return len;
	}

	s = libc_sd;
	lwip_write(s, &sysargs, sizeof(sysargs));

	i=0;
	while(i < len)
	{
		ret = lwip_write(s, (char*)buf+i, len-i);
		if (ret < 0) {
			spinlock_irqsave_unlock(&lwip_lock);
			return ret;
		}

		i += ret;
	}

	if (fd > 2) {
		ret = lwip_read(s, &i, sizeof(i));
		if (ret < 0)
			i = ret;
	} else i = len;

	spinlock_irqsave_unlock(&lwip_lock);

	return i;
}

ssize_t writev(int fildes, const struct iovec *iov, int iovcnt)
{
	return -ENOSYS;
}

ssize_t sys_sbrk(ssize_t incr)
{
	ssize_t ret;
	vma_t* heap = per_core(current_task)->heap;
	static spinlock_t heap_lock = SPINLOCK_INIT;

	if (BUILTIN_EXPECT(!heap, 0)) {
		LOG_ERROR("sys_sbrk: missing heap!\n");
		do_abort();
	}

	spinlock_lock(&heap_lock);

	ret = heap->end;

	// check heapp boundaries
	if ((heap->end >= HEAP_START) && (heap->end+incr < HEAP_START + HEAP_SIZE)) {
		heap->end += incr;

		// reserve VMA regions
		if (PAGE_FLOOR(heap->end) > PAGE_FLOOR(ret)) {
			// region is already reserved for the heap, we have to change the
			// property
			vma_free(PAGE_FLOOR(ret), PAGE_CEIL(heap->end));
			vma_add(PAGE_FLOOR(ret), PAGE_CEIL(heap->end), VMA_HEAP|VMA_USER);
		}
	} else ret = -ENOMEM;

	// allocation and mapping of new pages for the heap
	// is catched by the pagefault handler

	spinlock_unlock(&heap_lock);

	return ret;
}

typedef struct {
	const char* name;
	int flags;
	int mode;
	int ret;
} __attribute__((packed)) uhyve_open_t;

int sys_open(const char* name, int flags, int mode)
{
	if (is_uhyve()) {
		uhyve_open_t uhyve_open = {(const char*)virt_to_phys((size_t)name), flags, mode, -1};

		uhyve_send(UHYVE_PORT_OPEN, (unsigned)virt_to_phys((size_t) &uhyve_open));

		return uhyve_open.ret;
	}

	int s, i, ret, sysnr = __NR_open;
	size_t len;

	spinlock_irqsave_lock(&lwip_lock);
	if (libc_sd < 0) {
		ret = -EINVAL;
		goto out;
	}

	s = libc_sd;
	len = strlen(name)+1;

	//i = 0;
	//lwip_setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (char *) &i, sizeof(i));

	ret = lwip_write(s, &sysnr, sizeof(sysnr));
	if (ret < 0)
		goto out;

	ret = lwip_write(s, &len, sizeof(len));
	if (ret < 0)
		goto out;

	i=0;
	while(i<len)
	{
		ret = lwip_write(s, name+i, len-i);
		if (ret < 0)
			goto out;
		i += ret;
	}

	ret = lwip_write(s, &flags, sizeof(flags));
	if (ret < 0)
		goto out;

	ret = lwip_write(s, &mode, sizeof(mode));
	if (ret < 0)
		goto out;

	//i = 1;
	//lwip_setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (char *) &i, sizeof(i));

	lwip_read(s, &ret, sizeof(ret));

out:
	spinlock_irqsave_unlock(&lwip_lock);

	return ret;
}

typedef struct {
	int sysnr;
	int fd;
} __attribute__((packed)) sys_close_t;

typedef struct {
        int fd;
        int ret;
} __attribute__((packed)) uhyve_close_t;

int sys_close(int fd)
{
	if (is_uhyve()) {
		uhyve_close_t uhyve_close = {fd, -1};

		uhyve_send(UHYVE_PORT_CLOSE, (unsigned)virt_to_phys((size_t) &uhyve_close));

		return uhyve_close.ret;
	}

	int ret, s;
	sys_close_t sysargs = {__NR_close, fd};

	// do we have an LwIP file descriptor?
	if (fd & LWIP_FD_BIT) {
		ret = lwip_close(fd & ~LWIP_FD_BIT);
		if (ret < 0)
			return -errno;

		return 0;
	}

	spinlock_irqsave_lock(&lwip_lock);
	if (libc_sd < 0) {
		ret = 0;
		goto out;
	}

	s = libc_sd;
	ret = lwip_write(s, &sysargs, sizeof(sysargs));
	if (ret != sizeof(sysargs))
		goto out;
	lwip_read(s, &ret, sizeof(ret));

out:
	spinlock_irqsave_unlock(&lwip_lock);

	return ret;
}

int sys_spinlock_init(spinlock_t** lock)
{
	int ret;

	if (BUILTIN_EXPECT(!lock, 0))
		return -EINVAL;

	*lock = (spinlock_t*) kmalloc(sizeof(spinlock_t));
	if (BUILTIN_EXPECT(!(*lock), 0))
		return -ENOMEM;

	ret = spinlock_init(*lock);
	if (ret) {
		kfree(*lock);
		*lock = NULL;
	}

	return ret;
}

int sys_spinlock_destroy(spinlock_t* lock)
{
	int ret;

	if (BUILTIN_EXPECT(!lock, 0))
		return -EINVAL;

	ret = spinlock_destroy(lock);
	if (!ret)
		kfree(lock);

	return ret;
}

int sys_spinlock_lock(spinlock_t* lock)
{
	if (BUILTIN_EXPECT(!lock, 0))
		return -EINVAL;

	return spinlock_lock(lock);
}

int sys_spinlock_unlock(spinlock_t* lock)
{
	if (BUILTIN_EXPECT(!lock, 0))
		return -EINVAL;

	return spinlock_unlock(lock);
}

void sys_msleep(unsigned int ms)
{
	if (ms * TIMER_FREQ / 1000 > 0)
		timer_wait(ms * TIMER_FREQ / 1000);
	else if (ms > 0)
		udelay(ms * 1000);
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
	if (BUILTIN_EXPECT(!sem, 0))
		return -EINVAL;

	return sem_wait(sem, ms);
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

typedef struct {
	int fd;
	off_t offset;
	int whence;
} __attribute__((packed)) uhyve_lseek_t;

off_t sys_lseek(int fd, off_t offset, int whence)
{
	if (is_uhyve()) {
		uhyve_lseek_t uhyve_lseek = { fd, offset, whence };

		outportl(UHYVE_PORT_LSEEK, (unsigned)virt_to_phys((size_t) &uhyve_lseek));

		return uhyve_lseek.offset;
	}

	off_t off;
	sys_lseek_t sysargs = {__NR_lseek, fd, offset, whence};
	int s;

	spinlock_irqsave_lock(&lwip_lock);

	if (libc_sd < 0) {
		spinlock_irqsave_unlock(&lwip_lock);
		return -ENOSYS;
	}

	s = libc_sd;
	lwip_write(s, &sysargs, sizeof(sysargs));
	lwip_read(s, &off, sizeof(off));

	spinlock_irqsave_unlock(&lwip_lock);

	return off;
}

int sys_rcce_init(int session_id)
{
	int i, err = 0;
	size_t paddr = 0;

	if (is_single_kernel())
		return -ENOSYS;

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

	if (is_hbmem_available())
		paddr = hbmem_get_pages(RCCE_MPB_SIZE / PAGE_SIZE);
	else
		paddr = get_pages(RCCE_MPB_SIZE / PAGE_SIZE);
	if (BUILTIN_EXPECT(!paddr, 0))
	{
		err = -ENOMEM;
		goto out;
	}

	rcce_mpb[i].mpb[isle] = paddr;

out:
	islelock_unlock(rcce_lock);

	LOG_INFO("Create MPB for session %d at 0x%zx, using of slot %d\n", session_id, paddr, i);

	return err;
}

size_t sys_rcce_malloc(int session_id, int ue)
{
	size_t vaddr = 0;
	int i, counter = 0;

	if (is_single_kernel())
		return -ENOSYS;

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

	LOG_DEBUG("i = %d, counter = %d, max %d\n", i, counter, MAX_RCCE_SESSIONS);

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

	LOG_INFO("Map MPB of session %d at 0x%zx, using of slot %d, isle %d\n", session_id, vaddr, i, ue);

	if (isle == ue)
		memset((void*)vaddr, 0x0, RCCE_MPB_SIZE);

	return vaddr;

out:
	LOG_ERROR("Didn't find a valid MPB for session %d, isle %d\n", session_id, ue);

	return 0;
}

int sys_rcce_fini(int session_id)
{
	int i, j;
	int ret = 0;

	// we have to free the MPB

	if (is_single_kernel())
		return -ENOSYS;

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

	if (rcce_mpb[i].mpb[isle]) {
		if (is_hbmem_available())
			hbmem_put_pages(rcce_mpb[i].mpb[isle], RCCE_MPB_SIZE / PAGE_SIZE);
		else
			put_pages(rcce_mpb[i].mpb[isle], RCCE_MPB_SIZE / PAGE_SIZE);
	}
	rcce_mpb[i].mpb[isle] = 0;

	for(j=0; (j<MAX_ISLE) && !rcce_mpb[i].mpb[j]; j++) {
		PAUSE;
	}

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

void sys_yield(void)
{
#if 0
	check_workqueues();
#else
	if (BUILTIN_EXPECT(go_down, 0))
		shutdown_system();
	check_scheduling();
#endif
}

int sys_kill(tid_t dest, int signum)
{
	if(signum < 0) {
		return -EINVAL;
	}
	return hermit_kill(dest, signum);
}

int sys_signal(signal_handler_t handler)
{
	return hermit_signal(handler);
}
