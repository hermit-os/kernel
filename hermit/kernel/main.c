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
#include <hermit/string.h>
#include <hermit/time.h>
#include <hermit/tasks.h>
#include <hermit/processor.h>
#include <hermit/tasks.h>
#include <hermit/syscall.h>
#include <hermit/memory.h>
#include <hermit/spinlock.h>
#include <hermit/fs.h>
#include <asm/irq.h>
#include <asm/page.h>

#include <lwip/init.h>
#include <lwip/sys.h>
#include <lwip/stats.h>
#include <lwip/udp.h>
#include <lwip/tcp.h>
#include <lwip/tcpip.h>
#include <lwip/dhcp.h>
#include <lwip/netifapi.h>
#include <lwip/timers.h>
#include <netif/etharp.h>
#include <net/mmnif.h>

static struct netif	mmnif_netif;

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void kernel_start;
extern const void kernel_end;
extern const void bss_start;
extern const void bss_end;
extern const void percore_start;
extern const void percore_end0;
extern const void percore_end;
extern char __BUILD_DATE;

/* Page frame counters */
extern atomic_int64_t total_pages;
extern atomic_int64_t total_allocated_pages;
extern atomic_int64_t total_available_pages;

extern atomic_int32_t cpu_online;
extern atomic_int32_t possible_cpus;
extern int32_t isle;
extern int32_t possible_isles;

static int foo(void* arg)
{
	int i;

	for(i=0; i<5; i++) {
		kprintf("hello from %s\n", (char*) arg);
		sleep(1);
	}

	return 0;
}

static int hermit_init(void)
{
	uint32_t i;
	size_t sz = (size_t) &percore_end0 - (size_t) &percore_start;

	// initialize .bss section
	memset((void*)&bss_start, 0x00, ((size_t) &bss_end - (size_t) &bss_start));

	// initialize .percore section => copy first section to all other sections
	for(i=1; i<MAX_CORES; i++)
		memcpy((char*) &percore_start + i*sz, (char*) &percore_start, sz);

	koutput_init();
	system_init();
	irq_init();
	timer_init();
	multitasking_init();
	memory_init();
	initrd_init();

	return 0;
}

static void print_status(void)
{
	static spinlock_t status_lock = SPINLOCK_INIT;

	spinlock_lock(&status_lock);
	kprintf("CPU %d of isle %d is now online (CR0 0x%zx, CR4 0x%zx)\n", CORE_ID, isle, read_cr0(), read_cr4());
	spinlock_unlock(&status_lock);
}

static void tcpip_init_done(void* arg)
{
	sys_sem_t* sem = (sys_sem_t*)arg;

	kprintf("LwIP's tcpip thread has task id %d\n", per_core(current_task)->id);

	sys_sem_signal(sem);
}

static int init_netifs(void)
{
	struct ip_addr	ipaddr;
	struct ip_addr	netmask;
	struct ip_addr	gw;
	err_t		err;
	sys_sem_t	sem;

	if(sys_sem_new(&sem, 0) != ERR_OK)
		LWIP_ASSERT("Failed to create semaphore", 0);

	tcpip_init(tcpip_init_done, &sem);
	sys_sem_wait(&sem);
	kprintf("TCP/IP initialized.\n");
	sys_sem_free(&sem);

	/* Set network address variables */
        IP4_ADDR(&gw, 192,168,28,1);
        IP4_ADDR(&ipaddr, 192,168,28,isle+2);
        IP4_ADDR(&netmask, 255,255,255,0);

	/* register our Memory Mapped Virtual IP interface in the lwip stack
	 * and tell him how to use the interface:
	 *  - mmnif_dev : the device data storage
	 *  - ipaddr : the ip address wich should be used
	 *  - gw : the gateway wicht should be used
	 *  - mmnif_init : the initialization which has to be done in order to use our interface
	 *  - ip_input : tells him that he should use ip_input
	 *
	 * Note: Our drivers guarantee that the input function will be called in the context of the tcpip thread.
	 * => Therefore, we are able to use ip_input instead of tcpip_input */
        if ((err = netifapi_netif_add(&mmnif_netif, &ipaddr, &netmask, &gw, NULL, mmnif_init, ip_input)) != ERR_OK)
        {
                kprintf("Unable to add the intra network interface: err = %d\n", err);
                return -ENODEV;
        }

	/* tell lwip all initialization is done and we want to set it up */
	netifapi_netif_set_default(&mmnif_netif);
	netifapi_netif_set_up(&mmnif_netif);

	return 0;
}

#if MAX_CORES > 1
int smp_main(void)
{
	int32_t cpu = atomic_int32_inc(&cpu_online);

#ifdef CONFIG_TICKLESS
	disable_timer_irq();
#endif

	/* wait for the other cpus */
	while(atomic_int32_read(&cpu_online) < atomic_int32_read(&possible_cpus))
		PAUSE;

	print_status();

	//create_kernel_task(NULL, foo, "foo2", NORMAL_PRIO);

	while(1) {
		check_workqueues();
		HALT;
	}

	return 0;
}
#endif

// init task => creates all other tasks an initialize the LwIP
static int initd(void* arg)
{
	//char* argv1[] = {"/bin/hello", NULL};
	//char* argv2[] = {"/bin/jacobi", NULL};
	//char* argv3[] = {"/bin/stream", NULL};
	//char* argv4[] = {"/bin/thr_hello", NULL};

	init_netifs();

	//create_kernel_task(NULL, foo, "foo1", NORMAL_PRIO);
	//create_kernel_task(NULL, foo, "foo2", NORMAL_PRIO);
	//create_user_task(NULL, "/bin/hello", argv1, NORMAL_PRIO);
	//create_user_task(NULL, "/bin/jacobi", argv2, NORMAL_PRIO);
	//create_user_task(NULL, "/bin/jacobi", argv2, NORMAL_PRIO);
	//create_user_task(NULL, "/bin/stream", argv3, NORMAL_PRIO);
	//create_user_task(NULL, "/bin/thr_hello", argv4, NORMAL_PRIO);

	return 0;
}

int main(void)
{
	hermit_init();
	system_calibration(); // enables also interrupts

	atomic_int32_inc(&cpu_online);

	kprintf("This is Hermit %s, build date %u\n", VERSION, &__BUILD_DATE);
	kprintf("Isle %d of %d possible isles\n", isle, possible_isles);
	kprintf("Kernel starts at %p and ends at %p\n", &kernel_start, &kernel_end);
	kprintf("Per core data starts at %p and ends at %p\n", &percore_start, &percore_end);
	kprintf("Per core size 0x%zd\n", (size_t) &percore_end0 - (size_t) &percore_start);
	kprintf("Processor frequency: %u MHz\n", get_cpu_frequency());
	kprintf("Total memory: %zd MiB\n", atomic_int64_read(&total_pages) * PAGE_SIZE / (1024ULL*1024ULL));
	kprintf("Current allocated memory: %zd KiB\n", atomic_int64_read(&total_allocated_pages) * PAGE_SIZE / 1024ULL);
	kprintf("Current available memory: %zd MiB\n", atomic_int64_read(&total_available_pages) * PAGE_SIZE / (1024ULL*1024ULL));

#if 1
	kputs("Filesystem:\n");
	list_fs(fs_root, 1);
#endif

#ifdef CONFIG_TICKLESS
	disable_timer_irq();
#endif

	/* wait for the other cpus */
	while(atomic_int32_read(&cpu_online) < atomic_int32_read(&possible_cpus))
		PAUSE;

	print_status();

	create_kernel_task(NULL, initd, NULL, NORMAL_PRIO);

	while(1) {
		check_workqueues();
		HALT;
	}

	return 0;
}
