/* Copyright (c) 2015, IBM
 * Author(s): Dan Williams <djwillia@us.ibm.com>
 *            Ricardo Koller <kollerr@us.ibm.com>
 * Copyright (c) 2017, RWTH Aachen University
 * Author(s): Stefan Lankes <slankes@eonerc.rwth-aachen.de>
 *
 * Permission to use, copy, modify, and/or distribute this software
 * for any purpose with or without fee is hereby granted, provided
 * that the above copyright notice and this permission notice appear
 * in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL
 * WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
 * AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR
 * CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
 * OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
 * NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
 * CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/* We used several existing projects as guides
 * kvmtest.c: http://lwn.net/Articles/658512/
 * Solo5: https://github.com/Solo5/solo5
 */

/*
 * 15.1.2017: extend original version (https://github.com/Solo5/solo5)
 *            for HermitCore
 * 25.2.2017: add SMP support to enable more than one core
 * 24.4.2017: add checkpoint/restore support,
 *            remove memory limit
 */

#define _GNU_SOURCE

#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <stdbool.h>
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <signal.h>
#include <limits.h>
#include <pthread.h>
#include <semaphore.h>
#include <elf.h>
#include <err.h>
#include <poll.h>
#include <sys/wait.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/time.h>
#include <sys/eventfd.h>
#include <linux/const.h>
#include <linux/kvm.h>

#include "uhyve.h"
#include "uhyve-syscalls.h"
#include "uhyve-net.h"
#include "proxy.h"

static bool restart = false;
static pthread_t net_thread;
static int* vcpu_fds = NULL;
static pthread_mutex_t kvm_lock = PTHREAD_MUTEX_INITIALIZER;

extern bool verbose;

size_t guest_size = 0x20000000ULL;
bool full_checkpoint = false;
pthread_barrier_t barrier;
pthread_t* vcpu_threads = NULL;
uint8_t* klog = NULL;
uint8_t* guest_mem = NULL;
uint32_t no_checkpoint = 0;
uint32_t ncores = 1;
uint64_t elf_entry;
int kvm = -1, vmfd = -1, netfd = -1, efd = -1;
uint8_t* mboot = NULL;
__thread struct kvm_run *run = NULL;
__thread int vcpufd = -1;
__thread uint32_t cpuid = 0;
static sem_t net_sem;

int uhyve_argc = -1;
int uhyve_envc = -1;
char **uhyve_argv = NULL;
extern char **environ;
char **uhyve_envp = NULL;

typedef struct {
	int argc;
	int argsz[MAX_ARGC_ENVC];
	int envc;
	int envsz[MAX_ARGC_ENVC];
} __attribute__ ((packed)) uhyve_cmdsize_t;

typedef struct {
	char **argv;
	char **envp;
} __attribute__ ((packed)) uhyve_cmdval_t;

static uint64_t memparse(const char *ptr)
{
	// local pointer to end of parsed string
	char *endptr;

	// parse number
	uint64_t size = strtoull(ptr, &endptr, 0);

	// parse size extension, intentional fall-through
	switch (*endptr) {
	case 'E':
	case 'e':
		size <<= 10;
	case 'P':
	case 'p':
		size <<= 10;
	case 'T':
	case 't':
		size <<= 10;
	case 'G':
	case 'g':
		size <<= 10;
	case 'M':
	case 'm':
		size <<= 10;
	case 'K':
	case 'k':
		size <<= 10;
		endptr++;
	default:
		break;
	}

	return size;
}

// Just close file descriptor if not already done
static void close_fd(int* fd)
{
	if (*fd != -1) {
		close(*fd);
		*fd = -1;
	}
}

static void uhyve_exit(void* arg)
{
	//print_registers();

	if (pthread_mutex_trylock(&kvm_lock))
	{
		close_fd(&vcpufd);
		return;
	}

	// only the main thread will execute this
	if (vcpu_threads) {
		for(uint32_t i=0; i<ncores; i++) {
			if (pthread_self() == vcpu_threads[i])
				continue;

			pthread_kill(vcpu_threads[i], SIGTERM);
		}

		if (netfd > 0)
			pthread_kill(net_thread, SIGTERM);
	}

	close_fd(&vcpufd);
}

static void dump_log(void)
{
	if (klog && verbose)
	{
		fputs("\nDump kernel log:\n", stderr);
		fputs("================\n", stderr);
		fprintf(stderr, "%s\n", klog);
	}
}

static void uhyve_atexit(void)
{
	uhyve_exit(NULL);

	if (vcpu_threads) {
		for(uint32_t i = 0; i < ncores; i++) {
			if (pthread_self() == vcpu_threads[i])
				continue;
			pthread_join(vcpu_threads[i], NULL);
		}

		free(vcpu_threads);
	}

	if (vcpu_fds)
		free(vcpu_fds);

	dump_log();

	// clean up and close KVM
	close_fd(&vmfd);
	close_fd(&kvm);
}

static void* wait_for_packet(void* arg)
{
	int ret;
	struct pollfd fds = {	.fd = netfd,
				.events = POLLIN,
				.revents  = 0};

	while(1)
	{
		fds.revents = 0;

		ret = poll(&fds, 1, -1000);

		if (ret < 0 && errno == EINTR)
			continue;

		if (ret < 0)
			perror("poll()");
		else if (ret) {
			uint64_t event_counter = 1;
			write(efd, &event_counter, sizeof(event_counter));
			sem_wait(&net_sem);
		}
	}

	return NULL;
}

static inline void check_network(void)
{
	// should we start the network thread?
	if ((efd < 0) && (getenv("HERMIT_NETIF"))) {
		struct kvm_irqfd irqfd = {};

		efd = eventfd(0, 0);
		irqfd.fd = efd;
		irqfd.gsi = UHYVE_IRQ;
		kvm_ioctl(vmfd, KVM_IRQFD, &irqfd);

		sem_init(&net_sem, 0, 0);

		if (pthread_create(&net_thread, NULL, wait_for_packet, NULL))
			err(1, "unable to create thread");
	}
}

static int vcpu_loop(void)
{
	int ret;

	if (restart) {
		pthread_barrier_wait(&barrier);
		if (cpuid == 0)
			no_checkpoint++;
	}

	while (1) {
		ret = ioctl(vcpufd, KVM_RUN, NULL);

		if(ret == -1) {
			switch(errno) {
			case EINTR:
				continue;

			case EFAULT: {
				struct kvm_regs regs;
				kvm_ioctl(vcpufd, KVM_GET_REGS, &regs);
#ifdef __x86_64__
				err(1, "KVM: host/guest translation fault: rip=0x%llx", regs.rip);
#else
				err(1, "KVM: host/guest translation fault: elr_el1=0x%llx", regs.elr_el1);
#endif
			}

			default:
				err(1, "KVM: ioctl KVM_RUN in vcpu_loop failed");
				break;
			}
		}

		uint64_t port = 0;
		unsigned raddr = 0;

		/* handle requests */
		switch (run->exit_reason) {
		case KVM_EXIT_HLT:
			fprintf(stderr, "Guest has halted the CPU, this is considered as a normal exit.\n");
			return 0;

		case KVM_EXIT_MMIO:
			port = run->mmio.phys_addr;
			if (run->mmio.is_write)
				memcpy(&raddr, run->mmio.data, sizeof(raddr) /*run->mmio.len*/);
			//printf("KVM: handled KVM_EXIT_MMIO at 0x%lx (data %u)\n", port, raddr);

		case KVM_EXIT_IO:
			if (!port) {
				port = run->io.port;
				raddr = *((unsigned*)((size_t)run+run->io.data_offset));
			}

			//printf("port 0x%x\n", run->io.port);
			switch (port) {
			case UHYVE_PORT_WRITE: {
					uhyve_write_t* uhyve_write = (uhyve_write_t*) (guest_mem+raddr);

					uhyve_write->len = write(uhyve_write->fd, guest_mem+(size_t)uhyve_write->buf, uhyve_write->len);
					break;
				}

			case UHYVE_PORT_READ: {
					uhyve_read_t* uhyve_read = (uhyve_read_t*) (guest_mem+raddr);

					uhyve_read->ret = read(uhyve_read->fd, guest_mem+(size_t)uhyve_read->buf, uhyve_read->len);
					break;
				}

			case UHYVE_PORT_EXIT: {
					if (cpuid)
						pthread_exit((int*)(guest_mem+raddr));
					else
						exit(*(int*)(guest_mem+raddr));
					break;
				}

			case UHYVE_PORT_OPEN: {
					uhyve_open_t* uhyve_open = (uhyve_open_t*) (guest_mem+raddr);
					char rpath[PATH_MAX];

					// forbid to open the kvm device
					if (realpath((const char*)guest_mem+(size_t)uhyve_open->name, rpath) < 0)
						uhyve_open->ret = -1;
					else if (strcmp(rpath, "/dev/kvm") == 0)
						uhyve_open->ret = -1;
					else
						uhyve_open->ret = open((const char*)guest_mem+(size_t)uhyve_open->name, uhyve_open->flags, uhyve_open->mode);
					break;
				}

			case UHYVE_PORT_CLOSE: {
					uhyve_close_t* uhyve_close = (uhyve_close_t*) (guest_mem+raddr);

					if (uhyve_close->fd > 2)
						uhyve_close->ret = close(uhyve_close->fd);
					else
						uhyve_close->ret = 0;
					break;
				}

			case UHYVE_PORT_NETINFO: {
					uhyve_netinfo_t* uhyve_netinfo = (uhyve_netinfo_t*)(guest_mem+raddr);
					memcpy(uhyve_netinfo->mac_str, uhyve_get_mac(), 18);
					// guest configure the ethernet device => start network thread
					check_network();
					break;
				}

			case UHYVE_PORT_NETWRITE: {
					uhyve_netwrite_t* uhyve_netwrite = (uhyve_netwrite_t*)(guest_mem + raddr);
					uhyve_netwrite->ret = 0;
					ret = write(netfd, guest_mem + (size_t)uhyve_netwrite->data, uhyve_netwrite->len);
					if (ret >= 0) {
						uhyve_netwrite->ret = 0;
						uhyve_netwrite->len = ret;
					} else {
						uhyve_netwrite->ret = -1;
					}
					break;
				}

			case UHYVE_PORT_NETREAD: {
					uhyve_netread_t* uhyve_netread = (uhyve_netread_t*)(guest_mem + raddr);
					ret = read(netfd, guest_mem + (size_t)uhyve_netread->data, uhyve_netread->len);
					if (ret > 0) {
						uhyve_netread->len = ret;
						uhyve_netread->ret = 0;
					} else {
						uhyve_netread->ret = -1;
						sem_post(&net_sem);
					}
					break;
				}

			case UHYVE_PORT_NETSTAT: {
					uhyve_netstat_t* uhyve_netstat = (uhyve_netstat_t*)(guest_mem + raddr);
					char* str = getenv("HERMIT_NETIF");
					if (str)
						uhyve_netstat->status = 1;
					else
						uhyve_netstat->status = 0;
					break;
				}

			case UHYVE_PORT_LSEEK: {
					uhyve_lseek_t* uhyve_lseek = (uhyve_lseek_t*) (guest_mem+raddr);

					uhyve_lseek->offset = lseek(uhyve_lseek->fd, uhyve_lseek->offset, uhyve_lseek->whence);
					break;
				}

			case UHYVE_PORT_CMDSIZE: {
					int i;
					uhyve_cmdsize_t *val = (uhyve_cmdsize_t *) (guest_mem+raddr);

					val->argc = uhyve_argc;
					for(i=0; i<uhyve_argc; i++)
						val->argsz[i] = strlen(uhyve_argv[i]) + 1;

					val->envc = uhyve_envc;
					for(i=0; i<uhyve_envc; i++)
						val->envsz[i] = strlen(uhyve_envp[i]) + 1;

					break;
				}

			case UHYVE_PORT_CMDVAL: {
					int i;
					char **argv_ptr, **env_ptr;
					uhyve_cmdval_t *val = (uhyve_cmdval_t *) (guest_mem+raddr);

					/* argv */
					argv_ptr = (char **)(guest_mem + (size_t)val->argv);
					for(i=0; i<uhyve_argc; i++)
						strcpy(guest_mem + (size_t)argv_ptr[i], uhyve_argv[i]);

					/* env */
					env_ptr = (char **)(guest_mem + (size_t)val->envp);
					for(i=0; i<uhyve_envc; i++)
						strcpy(guest_mem + (size_t)env_ptr[i], uhyve_envp[i]);

					break;
				}

			default:
				err(1, "KVM: unhandled KVM_EXIT_IO / KVM_EXIT_MMIO at port 0x%lx\n", port);
				break;
			}
			break;

		case KVM_EXIT_FAIL_ENTRY:
			err(1, "KVM: entry failure: hw_entry_failure_reason=0x%llx\n",
				run->fail_entry.hardware_entry_failure_reason);
			break;

		case KVM_EXIT_INTERNAL_ERROR:
			err(1, "KVM: internal error exit: suberror = 0x%x\n", run->internal.suberror);
			break;

		case KVM_EXIT_SHUTDOWN:
			fprintf(stderr, "KVM: receive shutdown command\n");

		case KVM_EXIT_DEBUG:
			print_registers();
			dump_log();
			exit(EXIT_FAILURE);

		default:
			fprintf(stderr, "KVM: unhandled exit: exit_reason = 0x%x\n", run->exit_reason);
			exit(EXIT_FAILURE);
		}
	}

	close(vcpufd);
	vcpufd = -1;

	return 0;
}

static int vcpu_init(void)
{
	vcpu_fds[cpuid] = vcpufd = kvm_ioctl(vmfd, KVM_CREATE_VCPU, cpuid);

	/* Map the shared kvm_run structure and following data. */
	size_t mmap_size = (size_t) kvm_ioctl(kvm, KVM_GET_VCPU_MMAP_SIZE, NULL);

	if (mmap_size < sizeof(*run))
		err(1, "KVM: invalid VCPU_MMAP_SIZE: %zd", mmap_size);

	run = mmap(NULL, mmap_size, PROT_READ | PROT_WRITE, MAP_SHARED, vcpufd, 0);
	if (run == MAP_FAILED)
		err(1, "KVM: VCPU mmap failed");

	if (restart) {
		restore_cpu_state();
	} else {
		init_cpu_state(elf_entry);
	}

	return 0;
}

static void sigusr_handler(int signum)
{
	pthread_barrier_wait(&barrier);

	save_cpu_state();

	pthread_barrier_wait(&barrier);
}

static void* uhyve_thread(void* arg)
{
	size_t ret;
	struct sigaction sa;

	pthread_cleanup_push(uhyve_exit, NULL);

	cpuid = (size_t) arg;

	/* Install timer_handler as the signal handler for SIGVTALRM. */
	memset(&sa, 0x00, sizeof(sa));
	sa.sa_handler = &sigusr_handler;
	sigaction(SIGRTMIN, &sa, NULL);

	// create new cpu
	vcpu_init();

	// run cpu loop until thread gets killed
	ret = vcpu_loop();

	pthread_cleanup_pop(1);

	return (void*) ret;
}

void sigterm_handler(int signum)
{
	pthread_exit(0);
}

int uhyve_init(char *path)
{
	signal(SIGTERM, sigterm_handler);

	// register routine to close the VM
	atexit(uhyve_atexit);

	FILE* f = fopen("checkpoint/chk_config.txt", "r");
	if (f != NULL) {
		int tmp = 0;
		restart = true;

		fscanf(f, "number of cores: %u\n", &ncores);
		fscanf(f, "memory size: 0x%zx\n", &guest_size);
		fscanf(f, "checkpoint number: %u\n", &no_checkpoint);
		fscanf(f, "entry point: 0x%zx", &elf_entry);
		fscanf(f, "full checkpoint: %d", &tmp);
		full_checkpoint = tmp ? true : false;

		if (verbose)
			fprintf(stderr, "Restart from checkpoint %u (ncores %d, mem size 0x%zx)\n", no_checkpoint, ncores, guest_size);
		fclose(f);
	} else {
		const char* hermit_memory = getenv("HERMIT_MEM");
		if (hermit_memory)
			guest_size = memparse(hermit_memory);

		const char* hermit_cpus = getenv("HERMIT_CPUS");
		if (hermit_cpus)
			ncores = (uint32_t) atoi(hermit_cpus);

		const char* full_chk = getenv("HERMIT_FULLCHECKPOINT");
		if (full_chk && (strcmp(full_chk, "0") != 0))
			full_checkpoint = true;
	}

	vcpu_threads = (pthread_t*) calloc(ncores, sizeof(pthread_t));
	if (!vcpu_threads)
		err(1, "Not enough memory");

	vcpu_fds = (int*) calloc(ncores, sizeof(int));
	if (!vcpu_fds)
		err(1, "Not enough memory");

	kvm = open("/dev/kvm", O_RDWR | O_CLOEXEC);
	if (kvm < 0)
		err(1, "Could not open: /dev/kvm");

	/* Make sure we have the stable version of the API */
	int kvm_api_version = kvm_ioctl(kvm, KVM_GET_API_VERSION, NULL);
	if (kvm_api_version != 12)
		err(1, "KVM: API version is %d, uhyve requires version 12", kvm_api_version);

	/* Create the virtual machine */
	vmfd = kvm_ioctl(kvm, KVM_CREATE_VM, 0);

	init_kvm_arch();
	if (restart) {
		if (load_checkpoint(guest_mem, path) != 0)
			exit(EXIT_FAILURE);
	} else {
		if (load_kernel(guest_mem, path) != 0)
			exit(EXIT_FAILURE);
	}

	pthread_barrier_init(&barrier, NULL, ncores);
	cpuid = 0;

	// create first CPU, it will be the boot processor by default
	int ret = vcpu_init();

	const char* netif_str = getenv("HERMIT_NETIF");
	if (netif_str)
	{
		// TODO: strncmp for different network interfaces
		// for example tun/tap device or uhyvetap device
		netfd = uhyve_net_init(netif_str);
		if (netfd < 0)
			err(1, "unable to initialized network");
	}

	return ret;
}

int uhyve_loop(int argc, char **argv)
{
	const char* hermit_check = getenv("HERMIT_CHECKPOINT");
	int ts = 0, i = 0;

	/* argv[0] is 'proxy', do not count it */
	uhyve_argc = argc-1;
	uhyve_argv = &argv[1];
	uhyve_envp = environ;
	while(uhyve_envp[i] != NULL)
		i++;
	uhyve_envc = i;

	if (uhyve_argc > MAX_ARGC_ENVC) {
		fprintf(stderr, "uhyve downsiize envc from %d to %d\n", uhyve_argc, MAX_ARGC_ENVC);
		uhyve_argc = MAX_ARGC_ENVC;
	}

	if (uhyve_envc > MAX_ARGC_ENVC-1) {
		fprintf(stderr, "uhyve downsiize envc from %d to %d\n", uhyve_envc, MAX_ARGC_ENVC-1);
		uhyve_envc = MAX_ARGC_ENVC-1;
	}

	if(uhyve_argc > MAX_ARGC_ENVC || uhyve_envc > MAX_ARGC_ENVC) {
		fprintf(stderr, "uhyve cannot forward more than %d command line "
			"arguments or environment variables, please consider increasing "
				"the MAX_ARGC_ENVP cmake argument\n", MAX_ARGC_ENVC);
		return -1;
	}

	if (hermit_check)
		ts = atoi(hermit_check);

	*((uint32_t*) (mboot+0x24)) = ncores;

	// First CPU is special because it will boot the system. Other CPUs will
	// be booted linearily after the first one.
	vcpu_threads[0] = pthread_self();

	// start threads to create VCPUs
	for(size_t i = 1; i < ncores; i++)
		pthread_create(&vcpu_threads[i], NULL, uhyve_thread, (void*) i);

	if (ts > 0)
	{
		struct sigaction sa;
		struct itimerval timer;

		/* Install timer_handler as the signal handler for SIGVTALRM. */
		memset(&sa, 0x00, sizeof(sa));
		sa.sa_handler = &timer_handler;
		sigaction(SIGALRM, &sa, NULL);

		/* Configure the timer to expire after "ts" sec... */
		timer.it_value.tv_sec = ts;
		timer.it_value.tv_usec = 0;
		/* ... and every "ts" sec after that. */
		timer.it_interval.tv_sec = ts;
		timer.it_interval.tv_usec = 0;
		/* Start a virtual timer. It counts down whenever this process is executing. */
		setitimer(ITIMER_REAL, &timer, NULL);
	}

	// Run first CPU
	return vcpu_loop();
}
