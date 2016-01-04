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
#include <hermit/rcce.h>
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
#include <lwip/sockets.h>
#include <lwip/err.h>
#include <lwip/stats.h>
#include <netif/etharp.h>
#include <net/mmnif.h>

#define HERMIT_PORT	0x494F
#define HEMRIT_MAGIC	0x7E317

static struct netif	mmnif_netif;
static const int sobufsize = 131072;
volatile int8_t shutdown = 0;

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void kernel_start;
extern const void kernel_end;
extern const void hbss_start;
extern const void tls_start;
extern const void tls_end;
extern const void __bss_start;
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
extern int libc_sd;

islelock_t* rcce_lock = NULL;
rcce_mpb_t* rcce_mpb = NULL;

#if 0
static int foo(void* arg)
{
	int i;

	for(i=0; i<5; i++) {
		kprintf("hello from %s\n", (char*) arg);
		sleep(1);
	}

	return 0;
}
#endif

static int hermit_init(void)
{
	uint32_t i;
	size_t sz = (size_t) &percore_end0 - (size_t) &percore_start;

	// initialize .kbss sections
	memset((void*)&hbss_start, 0x00, ((size_t) &kernel_end - (size_t) &hbss_start));

	// initialize .percore section => copy first section to all other sections
	for(i=1; i<MAX_CORES; i++)
		memcpy((char*) &percore_start + i*sz, (char*) &percore_start, sz);

	koutput_init();
	system_init();
	irq_init();
	timer_init();
	multitasking_init();
	memory_init();

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

static int network_shutdown(void)
{
        mmnif_shutdown();
        netifapi_netif_set_down(&mmnif_netif);

        return 0;
}

#if MAX_CORES > 1
int smp_main(void)
{
	int32_t cpu = atomic_int32_inc(&cpu_online);

#ifdef DYNAMIC_TICKS
	enable_dynticks();
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

#if 0
static int init_rcce(void)
{
	size_t addr;

	addr = vma_alloc(PAGE_SIZE, VMA_READ|VMA_WRITE|VMA_CACHEABLE);
	if (BUILTIN_EXPECT(!addr, 0))
		return -ENOMEM;
	if (page_map(addr, phy_rcce_internals, 1, PG_GLOBAL|PG_RW)) {
		vma_free(addr, addr + PAGE_SIZE);
		return -ENOMEM;
	}

	rcce_lock = (islelock_t*) addr;
	rcce_mpb = (rcce_mpb_t*) (addr + CACHE_LINE*(RCCE_MAXNP+1));

	return 0;
}
#endif

int libc_start(int argc, char** argv, char** env);

// init task => creates all other tasks an initialize the LwIP
static int initd(void* arg)
{
	int s = -1, c = -1;
	int i, j, flag = 1;
	int len, err;
	int magic;
	struct sockaddr_in server, client;
	task_t* curr_task = per_core(current_task);
	size_t heap = 0x80000000;
	int argc, envc;
	char** argv = NULL;
	char **environ = NULL;

	kputs("Initd is running\n");

	// setup heap
	if (!curr_task->heap)
		curr_task->heap = (vma_t*) kmalloc(sizeof(vma_t));

	if (BUILTIN_EXPECT(!curr_task->heap, 0)) {
		kprintf("load_task: heap is missing!\n");
		return -ENOMEM;
	}

	curr_task->heap->flags = VMA_HEAP|VMA_USER;
	curr_task->heap->start = PAGE_FLOOR(heap);
	curr_task->heap->end = PAGE_FLOOR(heap);

	//create_kernel_task(NULL, foo, "foo1", NORMAL_PRIO);
	//create_kernel_task(NULL, foo, "foo2", NORMAL_PRIO);

	init_netifs();

	// do we have a thread local storage?
	if (((size_t) &tls_end - (size_t) &tls_start) > 0) {
		char* tls_addr = NULL;

		curr_task->tls_addr = (size_t) &tls_start;
		curr_task->tls_size = (size_t) &tls_end - (size_t) &tls_start;

		// TODO: free TLS after termination
		tls_addr = kmalloc(curr_task->tls_size);
		if (BUILTIN_EXPECT(!tls_addr, 0)) {
			kprintf("load_task: heap is missing!\n");
			kfree(curr_task->heap);
			return -ENOMEM;
		}

		memcpy((void*) tls_addr, (void*) curr_task->tls_addr, curr_task->tls_size);

		// set fs register to the TLS segment
		set_tls((size_t) tls_addr);
		kprintf("Task %d set fs to 0x%zx\n", curr_task->id, tls_addr);
	} else set_tls(0); // no TLS => clear fs register

	//init_rcce();

	s = socket(PF_INET , SOCK_STREAM , 0);
	if (s < 0) {
		kprintf("socket failed: %d\n", server);
		return -1;
	}

	// prepare the sockaddr_in structure
	memset((char *) &server, 0x00, sizeof(server));
	server.sin_family = AF_INET;
	server.sin_addr.s_addr = INADDR_ANY;
	server.sin_port = htons(HERMIT_PORT);

	if ((err = bind(s, (struct sockaddr *) &server, sizeof(server))) < 0)
	{
		kprintf("bind failed: %d\n", errno);
		closesocket(s);
		return -1;
	}

	if ((err = listen(s, 2)) < 0)
	{
		kprintf("listen failed: %d\n", errno);
		closesocket(s);
		return -1;
	}

	len = sizeof(struct sockaddr_in);

	kputs("TCP server listening.\n");

	if ((c = accept(s, (struct sockaddr *)&client, (socklen_t*)&len)) < 0)
	{
		kprintf("accept faild: %d\n", errno);
		closesocket(s);
		return -1;
	}

	kputs("Establish IP connection\n");

	setsockopt(c, SOL_SOCKET, SO_RCVBUF, (char *) &sobufsize, sizeof(sobufsize));
	setsockopt(c, SOL_SOCKET, SO_SNDBUF, (char *) &sobufsize, sizeof(sobufsize));
	setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (char *) &flag, sizeof(flag));

	read(c, &magic, sizeof(magic));
	if (magic != HEMRIT_MAGIC)
	{
		kprintf("Invalid magic number %d\n", magic);
		closesocket(c);
		return -1;
	}

	err = read(c, &argc, sizeof(argc));
	if (err != sizeof(argc))
		goto out;

	argv = kmalloc((argc+1)*sizeof(char*));
	if (!argv)
		goto out;
	memset(argv, 0x00, (argc+1)*sizeof(char*));

	for(i=0; i<argc; i++)
	{
		err = read(c, &len, sizeof(len));
		if (err != sizeof(len))
			goto out;

		argv[i] = kmalloc(len);
		if (!argv[i])
			goto out;

		j = 0;
		while(j < len) {
			err = read(c, argv[i]+j, len-j);
			if (err < 0)
				goto out;
			j += err;
		}

	}

	err = read(c, &envc, sizeof(envc));
	if (err != sizeof(envc))
		goto out;

	environ = kmalloc((envc+1)*sizeof(char**));
	if (!environ)
		goto out;
	memset(environ, 0x00, (envc+1)*sizeof(char*));

	for(i=0; i<envc; i++)
	{
		err = read(c, &len, sizeof(len));
		if (err != sizeof(len))
			goto out;

		environ[i] = kmalloc(len);
		if (!environ[i])
			goto out;

		j = 0;
		while(j < len) {
			err = read(c, environ[i]+j, len-j);
			if (err < 0)
				goto out;
			j += err;
		}
	}

	// call user code
	libc_sd = c;
	libc_start(argc, argv, environ);

out:
	if (argv) {
		for(i=0; i<argc; i++) {
			if (argv[i])
				kfree(argv[i]);
		}

		kfree(argv);
	}

	if (environ) {
		i = 0;
		while(environ[i]) {
			kfree(environ[i]);
			i++;
		}

		kfree(environ);
	}

	if (c > 0)
		closesocket(c);
	libc_sd = -1;

	if (s > 0)
		closesocket(s);

	//network_shutdown();

	return 0;
}

int hermit_main(void)
{
	hermit_init();
	system_calibration(); // enables also interrupts

	atomic_int32_inc(&cpu_online);

	kprintf("This is Hermit %s, build date %u\n", VERSION, &__DATE__);
	kprintf("Isle %d of %d possible isles\n", isle, possible_isles);
	kprintf("Kernel starts at %p and ends at %p\n", &kernel_start, &kernel_end);
	kprintf("TLS image starts at %p and ends at %p\n", &tls_start, &tls_end);
	kprintf("Kernel BBS starts at %p and ends at %p\n", &hbss_start, &kernel_end);
	kprintf("Per core data starts at %p and ends at %p\n", &percore_start, &percore_end);
	kprintf("Per core size 0x%zd\n", (size_t) &percore_end0 - (size_t) &percore_start);
	kprintf("Processor frequency: %u MHz\n", get_cpu_frequency());
	kprintf("Total memory: %zd MiB\n", atomic_int64_read(&total_pages) * PAGE_SIZE / (1024ULL*1024ULL));
	kprintf("Current allocated memory: %zd KiB\n", atomic_int64_read(&total_allocated_pages) * PAGE_SIZE / 1024ULL);
	kprintf("Current available memory: %zd MiB\n", atomic_int64_read(&total_available_pages) * PAGE_SIZE / (1024ULL*1024ULL));

#if 0
	print_pci_adapters();
#endif

#ifdef DYNAMIC_TICKS
	enable_dynticks();
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
