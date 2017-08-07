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
#include <assert.h>
#include <pthread.h>
#include <elf.h>
#include <err.h>
#include <sys/wait.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/time.h>
#include <linux/const.h>
#include <linux/kvm.h>
#include <asm/msr-index.h>
#include <asm/mman.h>

#include "uhyve-cpu.h"
#include "uhyve-syscalls.h"
#include "proxy.h"

// define this macro to create checkpoints with KVM's dirty log
//#define USE_DIRTY_LOG

#define MAX_FNAME	256
#define MAX_MSR_ENTRIES	25

#define GUEST_OFFSET		0x0
#define CPUID_FUNC_PERFMON	0x0A
#define GUEST_PAGE_SIZE		0x200000   /* 2 MB pages in guest */

#define BOOT_GDT	0x1000
#define BOOT_INFO	0x2000
#define BOOT_PML4	0x10000
#define BOOT_PDPTE	0x11000
#define BOOT_PDE	0x12000

#define BOOT_GDT_NULL	0
#define BOOT_GDT_CODE	1
#define BOOT_GDT_DATA	2
#define BOOT_GDT_MAX	3

#define KVM_32BIT_MAX_MEM_SIZE	(1ULL << 32)
#define KVM_32BIT_GAP_SIZE	(768 << 20)
#define KVM_32BIT_GAP_START	(KVM_32BIT_MAX_MEM_SIZE - KVM_32BIT_GAP_SIZE)

/// Page offset bits
#define PAGE_BITS			12
#define PAGE_2M_BITS	21
#define PAGE_SIZE			(1L << PAGE_BITS)
/// Mask the page address without page map flags and XD flag
#if 0
#define PAGE_MASK		((~0L) << PAGE_BITS)
#define PAGE_2M_MASK		(~0L) << PAGE_2M_BITS)
#else
#define PAGE_MASK			(((~0L) << PAGE_BITS) & ~PG_XD)
#define PAGE_2M_MASK	(((~0L) << PAGE_2M_BITS) & ~PG_XD)
#endif

// Page is present
#define PG_PRESENT		(1 << 0)
// Page is read- and writable
#define PG_RW			(1 << 1)
// Page is addressable from userspace
#define PG_USER			(1 << 2)
// Page write through is activated
#define PG_PWT			(1 << 3)
// Page cache is disabled
#define PG_PCD			(1 << 4)
// Page was recently accessed (set by CPU)
#define PG_ACCESSED		(1 << 5)
// Page is dirty due to recent write-access (set by CPU)
#define PG_DIRTY		(1 << 6)
// Huge page: 4MB (or 2MB, 1GB)
#define PG_PSE			(1 << 7)
// Page attribute table
#define PG_PAT			PG_PSE
#if 1
/* @brief Global TLB entry (Pentium Pro and later)
 *
 * HermitCore is a single-address space operating system
 * => CR3 never changed => The flag isn't required for HermitCore
 */
#define PG_GLOBAL		0
#else
#define PG_GLOBAL		(1 << 8)
#endif
// This table is a self-reference and should skipped by page_map_copy()
#define PG_SELF			(1 << 9)

/// Disable execution for this page
#define PG_XD			(1L << 63)

#define BITS					64
#define PHYS_BITS			52
#define VIRT_BITS			48
#define PAGE_MAP_BITS	9
#define PAGE_LEVELS		4

#define kvm_ioctl(fd, cmd, arg) ({ \
	const int ret = ioctl(fd, cmd, arg); \
	if(ret == -1) \
		err(1, "KVM: ioctl " #cmd " failed"); \
	ret; \
	})

static bool restart = false;
static bool cap_tsc_deadline = false;
static bool cap_irqchip = false;
static bool cap_adjust_clock_stable = false;
static bool verbose = false;
static bool full_checkpoint = false;
static uint32_t ncores = 1;
static uint8_t* guest_mem = NULL;
static uint8_t* klog = NULL;
static uint8_t* mboot = NULL;
static size_t guest_size = 0x20000000ULL;
static uint64_t elf_entry;
static pthread_t* vcpu_threads = NULL;
static int* vcpu_fds = NULL;
static int kvm = -1, vmfd = -1;
static uint32_t no_checkpoint = 0;
static pthread_mutex_t kvm_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_barrier_t barrier;
static __thread struct kvm_run *run = NULL;
static __thread int vcpufd = -1;
static __thread uint32_t cpuid = 0;

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
static inline void close_fd(int* fd)
{
	if (*fd != -1) {
		close(*fd);
		*fd = -1;
	}
}

static void uhyve_exit(void* arg)
{
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
	}

	close_fd(&vcpufd);
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

	if (klog && verbose)
	{
		fputs("\nDump kernel log:\n", stderr);
		fputs("================\n", stderr);
		fprintf(stderr, "%s\n", klog);
	}

	// clean up and close KVM
	close_fd(&vmfd);
	close_fd(&kvm);
}

static uint32_t get_cpufreq(void)
{
	char line[128];
	uint32_t freq = 0;
	char* match;

	FILE* fp = fopen("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq", "r");
	if (fp != NULL) {
		if (fgets(line, sizeof(line), fp) != NULL) {
			// cpuinfo_max_freq is in kHz
			freq = (uint32_t) atoi(line) / 1000;
		}

		fclose(fp);
	} else if( (fp = fopen("/proc/cpuinfo", "r")) ) {
		// Resorting to /proc/cpuinfo, however on most systems this will only
		// return the current frequency that might change over time.
		// Currently only needed when running inside a VM

		// read until we find the line indicating cpu frequency
		while(fgets(line, sizeof(line), fp) != NULL) {
			match = strstr(line, "cpu MHz");

			if(match != NULL) {
				// advance pointer to beginning of number
				while( ((*match < '0') || (*match > '9')) && (*match != '\0') )
					match++;

				freq = (uint32_t) atoi(match);
				break;
			}
		}

		fclose(fp);
	}

	return freq;
}

static ssize_t pread_in_full(int fd, void *buf, size_t count, off_t offset)
{
	ssize_t total = 0;
	char *p = buf;

	if (count > SSIZE_MAX) {
		errno = E2BIG;
		return -1;
	}

	while (count > 0) {
		ssize_t nr;

		nr = pread(fd, p, count, offset);
		if (nr == 0)
			return total;
		else if (nr == -1 && errno == EINTR)
			continue;
		else if (nr == -1)
			return -1;

		count -= nr;
		total += nr;
		p += nr;
		offset += nr;
	}

	return total;
}

static int load_kernel(uint8_t* mem, char* path)
{
	Elf64_Ehdr hdr;
	Elf64_Phdr *phdr = NULL;
	size_t buflen;
	int fd, ret;
	int first_load = 1;

	fd = open(path, O_RDONLY);
	if (fd == -1)
	{
		perror("Unable to open file");
		return -1;
	}

	ret = pread_in_full(fd, &hdr, sizeof(hdr), 0);
	if (ret < 0)
		goto out;

	//  check if the program is a HermitCore file
	if (hdr.e_ident[EI_MAG0] != ELFMAG0
	    || hdr.e_ident[EI_MAG1] != ELFMAG1
	    || hdr.e_ident[EI_MAG2] != ELFMAG2
	    || hdr.e_ident[EI_MAG3] != ELFMAG3
	    || hdr.e_ident[EI_CLASS] != ELFCLASS64
	    || hdr.e_ident[EI_OSABI] != HERMIT_ELFOSABI
	    || hdr.e_type != ET_EXEC || hdr.e_machine != EM_X86_64) {
		fprintf(stderr, "Inavlide HermitCore file!\n");
		goto out;
	}

	elf_entry = hdr.e_entry;

	buflen = hdr.e_phentsize * hdr.e_phnum;
	phdr = malloc(buflen);
	if (!phdr) {
		fprintf(stderr, "Not enough memory\n");
		goto out;
	}

	ret = pread_in_full(fd, phdr, buflen, hdr.e_phoff);
	if (ret < 0)
		goto out;

	/*
	 * Load all segments with type "LOAD" from the file at offset
	 * p_offset, and copy that into in memory.
	 */
	for (Elf64_Half ph_i = 0; ph_i < hdr.e_phnum; ph_i++)
	{
		uint64_t paddr = phdr[ph_i].p_paddr;
		size_t offset = phdr[ph_i].p_offset;
		size_t filesz = phdr[ph_i].p_filesz;
		size_t memsz = phdr[ph_i].p_memsz;

		if (phdr[ph_i].p_type != PT_LOAD)
			continue;

		//printf("Kernel location 0x%zx, file size 0x%zx, memory size 0x%zx\n", paddr, filesz, memsz);

		ret = pread_in_full(fd, mem+paddr-GUEST_OFFSET, filesz, offset);
		if (ret < 0)
			goto out;
		if (!klog)
			klog = mem+paddr+0x5000-GUEST_OFFSET;
		if (!mboot)
			mboot = mem+paddr-GUEST_OFFSET;

		if (first_load) {
			first_load = 0;

			// initialize kernel
			*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0x08)) = paddr; // physical start address
			*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0x10)) = guest_size;   // physical limit
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x18)) = get_cpufreq();
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x24)) = 1; // number of used cpus
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x30)) = 0; // apicid
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x60)) = 1; // numa nodes
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x94)) = 1; // announce uhyve
		}
		*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0x38)) += memsz; // total kernel size
	}

out:
	if (phdr)
		free(phdr);

	close(fd);

	return 0;
}

static int load_checkpoint(uint8_t* mem, char* path)
{
	char fname[MAX_FNAME];
	size_t location;
	size_t paddr = elf_entry;
	int ret;
	struct timeval begin, end;
	uint32_t i;

	if (verbose)
		gettimeofday(&begin, NULL);

	if (!klog)
		klog = mem+paddr+0x5000-GUEST_OFFSET;
	if (!mboot)
		mboot = mem+paddr-GUEST_OFFSET;


#ifdef USE_DIRTY_LOG
	/*
	 * if we use KVM's dirty page logging, we have to load
	 * the elf image because most parts are readonly sections
	 * and aren't able to detect by KVM's dirty page logging
	 * technique.
	 */
	ret = load_kernel(mem, path);
	if (ret)
		return ret;
#endif

	i = full_checkpoint ? no_checkpoint : 0;
	for(; i<=no_checkpoint; i++)
	{
		snprintf(fname, MAX_FNAME, "checkpoint/chk%u_mem.dat", i);

		FILE* f = fopen(fname, "r");
		if (f == NULL)
			return -1;

		/*struct kvm_irqchip irqchip;
		if (fread(&irqchip, sizeof(irqchip), 1, f) != 1)
			err(1, "fread failed");
		if (cap_irqchip && (i == no_checkpoint-1))
			kvm_ioctl(vmfd, KVM_SET_IRQCHIP, &irqchip);*/

		struct kvm_clock_data clock;
		if (fread(&clock, sizeof(clock), 1, f) != 1)
			err(1, "fread failed");
		// only the last checkpoint has to set the clock
		if (cap_adjust_clock_stable && (i == no_checkpoint)) {
			struct kvm_clock_data data = {};

			data.clock = clock.clock;
			kvm_ioctl(vmfd, KVM_SET_CLOCK, &data);
		}

#if 0
		if (fread(guest_mem, guest_size, 1, f) != 1)
			err(1, "fread failed");
#else

		while (fread(&location, sizeof(location), 1, f) == 1) {
			//printf("location 0x%zx\n", location);
			if (location & PG_PSE)
				ret = fread((size_t*) (mem + (location & PAGE_2M_MASK)), (1UL << PAGE_2M_BITS), 1, f);
			else
				ret = fread((size_t*) (mem + (location & PAGE_MASK)), (1UL << PAGE_BITS), 1, f);

			if (ret != 1) {
				fprintf(stderr, "Unable to read checkpoint: ret = %d", ret);
				err(1, "fread failed");
			}
		}
#endif

		fclose(f);
	}

	if (verbose) {
		gettimeofday(&end, NULL);
		size_t msec = (end.tv_sec - begin.tv_sec) * 1000;
		msec += (end.tv_usec - begin.tv_usec) / 1000;
		fprintf(stderr, "Load checkpoint %u in %zd ms\n", no_checkpoint, msec);
	}

	return 0;
}

static inline void show_dtable(const char *name, struct kvm_dtable *dtable)
{
	fprintf(stderr, " %s                 %016zx  %08hx\n", name, (size_t) dtable->base, (uint16_t) dtable->limit);
}

static inline void show_segment(const char *name, struct kvm_segment *seg)
{
	fprintf(stderr, " %s       %04hx      %016zx  %08x  %02hhx    %x %x   %x  %x %x %x %x\n",
		name, (uint16_t) seg->selector, (size_t) seg->base, (uint32_t) seg->limit,
		(uint8_t) seg->type, seg->present, seg->dpl, seg->db, seg->s, seg->l, seg->g, seg->avl);
}

static void show_registers(int id, struct kvm_regs* regs, struct kvm_sregs* sregs)
{
	size_t cr0, cr2, cr3;
	size_t cr4, cr8;
	size_t rax, rbx, rcx;
	size_t rdx, rsi, rdi;
	size_t rbp,  r8,  r9;
	size_t r10, r11, r12;
	size_t r13, r14, r15;
	size_t rip, rsp;
	size_t rflags;
	int i;

	rflags = regs->rflags;
	rip = regs->rip; rsp = regs->rsp;
	rax = regs->rax; rbx = regs->rbx; rcx = regs->rcx;
	rdx = regs->rdx; rsi = regs->rsi; rdi = regs->rdi;
	rbp = regs->rbp; r8  = regs->r8;  r9  = regs->r9;
	r10 = regs->r10; r11 = regs->r11; r12 = regs->r12;
	r13 = regs->r13; r14 = regs->r14; r15 = regs->r15;

	fprintf(stderr, "\n Dump state of CPU %d\n", id);
	fprintf(stderr, "\n Registers:\n");
	fprintf(stderr, " ----------\n");
	fprintf(stderr, " rip: %016zx   rsp: %016zx flags: %016zx\n", rip, rsp, rflags);
	fprintf(stderr, " rax: %016zx   rbx: %016zx   rcx: %016zx\n", rax, rbx, rcx);
	fprintf(stderr, " rdx: %016zx   rsi: %016zx   rdi: %016zx\n", rdx, rsi, rdi);
	fprintf(stderr, " rbp: %016zx    r8: %016zx    r9: %016zx\n", rbp, r8,  r9);
	fprintf(stderr, " r10: %016zx   r11: %016zx   r12: %016zx\n", r10, r11, r12);
	fprintf(stderr, " r13: %016zx   r14: %016zx   r15: %016zx\n", r13, r14, r15);

	cr0 = sregs->cr0; cr2 = sregs->cr2; cr3 = sregs->cr3;
	cr4 = sregs->cr4; cr8 = sregs->cr8;

	fprintf(stderr, " cr0: %016zx   cr2: %016zx   cr3: %016zx\n", cr0, cr2, cr3);
	fprintf(stderr, " cr4: %016zx   cr8: %016zx\n", cr4, cr8);
	fprintf(stderr, "\n Segment registers:\n");
	fprintf(stderr,   " ------------------\n");
	fprintf(stderr, " register  selector  base              limit     type  p dpl db s l g avl\n");
	show_segment("cs ", &sregs->cs);
	show_segment("ss ", &sregs->ss);
	show_segment("ds ", &sregs->ds);
	show_segment("es ", &sregs->es);
	show_segment("fs ", &sregs->fs);
	show_segment("gs ", &sregs->gs);
	show_segment("tr ", &sregs->tr);
	show_segment("ldt", &sregs->ldt);
	show_dtable("gdt", &sregs->gdt);
	show_dtable("idt", &sregs->idt);

	fprintf(stderr, "\n APIC:\n");
	fprintf(stderr,   " -----\n");
	fprintf(stderr, " efer: %016zx  apic base: %016zx\n",
		(size_t) sregs->efer, (size_t) sregs->apic_base);

	fprintf(stderr, "\n Interrupt bitmap:\n");
	fprintf(stderr,   " -----------------\n");
	for (i = 0; i < (KVM_NR_INTERRUPTS + 63) / 64; i++)
		fprintf(stderr, " %016zx", (size_t) sregs->interrupt_bitmap[i]);
	fprintf(stderr, "\n");
}

static void print_registers(void)
{
	struct kvm_regs regs;
	struct kvm_sregs sregs;

	kvm_ioctl(vcpufd, KVM_GET_SREGS, &sregs);
	kvm_ioctl(vcpufd, KVM_GET_REGS, &regs);

	show_registers(cpuid, &regs, &sregs);
}

/// Filter CPUID functions that are not supported by the hypervisor and enable
/// features according to our needs.
static void filter_cpuid(struct kvm_cpuid2 *kvm_cpuid)
{
	for (uint32_t i = 0; i < kvm_cpuid->nent; i++) {
		struct kvm_cpuid_entry2 *entry = &kvm_cpuid->entries[i];

		switch (entry->function) {
		case 1:
			// CPUID to define basic cpu features
			entry->ecx |= (1U << 31); // propagate that we are running on a hypervisor
			if (cap_tsc_deadline)
				entry->ecx |= (1U << 24); // enable TSC deadline feature
			entry->edx |= (1U <<  5); // enable msr support
			break;

		case CPUID_FUNC_PERFMON:
			// disable it
			entry->eax	= 0x00;
			break;

		default:
			// Keep the CPUID function as-is
			break;
		};
	}
}

static void setup_system_64bit(struct kvm_sregs *sregs)
{
	sregs->cr0 |= X86_CR0_PE;
	sregs->efer |= EFER_LME;
}

static void setup_system_page_tables(struct kvm_sregs *sregs, uint8_t *mem)
{
	uint64_t *pml4 = (uint64_t *) (mem + BOOT_PML4);
	uint64_t *pdpte = (uint64_t *) (mem + BOOT_PDPTE);
	uint64_t *pde = (uint64_t *) (mem + BOOT_PDE);
	uint64_t paddr;

	/*
	 * For simplicity we currently use 2MB pages and only a single
	 * PML4/PDPTE/PDE.
	 */

	memset(pml4, 0x00, 4096);
	memset(pdpte, 0x00, 4096);
	memset(pde, 0x00, 4096);

	*pml4 = BOOT_PDPTE | (X86_PDPT_P | X86_PDPT_RW);
	*pdpte = BOOT_PDE | (X86_PDPT_P | X86_PDPT_RW);
	for (paddr = 0; paddr < 0x20000000ULL; paddr += GUEST_PAGE_SIZE, pde++)
		*pde = paddr | (X86_PDPT_P | X86_PDPT_RW | X86_PDPT_PS);

	sregs->cr3 = BOOT_PML4;
	sregs->cr4 |= X86_CR4_PAE;
	sregs->cr0 |= X86_CR0_PG;
}

static void setup_system_gdt(struct kvm_sregs *sregs,
                             uint8_t *mem,
                             uint64_t off)
{
	uint64_t *gdt = (uint64_t *) (mem + off);
	struct kvm_segment data_seg, code_seg;

	/* flags, base, limit */
	gdt[BOOT_GDT_NULL] = GDT_ENTRY(0, 0, 0);
	gdt[BOOT_GDT_CODE] = GDT_ENTRY(0xA09B, 0, 0xFFFFF);
	gdt[BOOT_GDT_DATA] = GDT_ENTRY(0xC093, 0, 0xFFFFF);

	sregs->gdt.base = off;
	sregs->gdt.limit = (sizeof(uint64_t) * BOOT_GDT_MAX) - 1;

	GDT_TO_KVM_SEGMENT(code_seg, gdt, BOOT_GDT_CODE);
	GDT_TO_KVM_SEGMENT(data_seg, gdt, BOOT_GDT_DATA);

	sregs->cs = code_seg;
	sregs->ds = data_seg;
	sregs->es = data_seg;
	sregs->fs = data_seg;
	sregs->gs = data_seg;
	sregs->ss = data_seg;
}

static void setup_system(int vcpufd, uint8_t *mem, uint32_t id)
{
	static struct kvm_sregs sregs;

	// all cores use the same startup code
	// => all cores use the same sregs
	// => only the boot processor has to initialize sregs
	if (id == 0) {
		kvm_ioctl(vcpufd, KVM_GET_SREGS, &sregs);

		/* Set all cpu/mem system structures */
		setup_system_gdt(&sregs, mem, BOOT_GDT);
		setup_system_page_tables(&sregs, mem);
		setup_system_64bit(&sregs);
	}

	kvm_ioctl(vcpufd, KVM_SET_SREGS, &sregs);
}

static void setup_cpuid(int kvm, int vcpufd)
{
	struct kvm_cpuid2 *kvm_cpuid;
	unsigned int max_entries = 100;

	// allocate space for cpuid we get from KVM
	kvm_cpuid = calloc(1, sizeof(*kvm_cpuid) + (max_entries * sizeof(kvm_cpuid->entries[0])));
	kvm_cpuid->nent = max_entries;

	kvm_ioctl(kvm, KVM_GET_SUPPORTED_CPUID, kvm_cpuid);

	// set features
	filter_cpuid(kvm_cpuid);
	kvm_ioctl(vcpufd, KVM_SET_CPUID2, kvm_cpuid);

	free(kvm_cpuid);
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
				err(1, "KVM: host/guest translation fault: rip=0x%llx", regs.rip);
			}

			default:
				err(1, "KVM: ioctl KVM_RUN in vcpu_loop failed");
				break;
			}
		}

		/* handle requests */
		switch (run->exit_reason) {
		case KVM_EXIT_HLT:
			fprintf(stderr, "Guest has halted the CPU, this is considered as a normal exit.\n");
			return 0;

		case KVM_EXIT_MMIO:
			err(1, "KVM: unhandled KVM_EXIT_MMIO at 0x%llx\n", run->mmio.phys_addr);
			break;

		case KVM_EXIT_IO:
			//printf("port 0x%x\n", run->io.port);
			switch (run->io.port) {
			case UHYVE_PORT_WRITE: {
					unsigned data = *((unsigned*)((size_t)run+run->io.data_offset));
					uhyve_write_t* uhyve_write = (uhyve_write_t*) (guest_mem+data);

					uhyve_write->len = write(uhyve_write->fd, guest_mem+(size_t)uhyve_write->buf, uhyve_write->len);
					break;
				}

			case UHYVE_PORT_READ: {
					unsigned data = *((unsigned*)((size_t)run+run->io.data_offset));
					uhyve_read_t* uhyve_read = (uhyve_read_t*) (guest_mem+data);

					uhyve_read->ret = read(uhyve_read->fd, guest_mem+(size_t)uhyve_read->buf, uhyve_read->len);
					break;
				}

			case UHYVE_PORT_EXIT: {
					unsigned data = *((unsigned*)((size_t)run+run->io.data_offset));

					if (cpuid)
						pthread_exit((int*)(guest_mem+data));
					else
						exit(*(int*)(guest_mem+data));
					break;
				}

			case UHYVE_PORT_OPEN: {
					unsigned data = *((unsigned*)((size_t)run+run->io.data_offset));
					uhyve_open_t* uhyve_open = (uhyve_open_t*) (guest_mem+data);

					uhyve_open->ret = open((const char*)guest_mem+(size_t)uhyve_open->name, uhyve_open->flags, uhyve_open->mode);
					break;
				}

			case UHYVE_PORT_CLOSE: {
					unsigned data = *((unsigned*)((size_t)run+run->io.data_offset));
					uhyve_close_t* uhyve_close = (uhyve_close_t*) (guest_mem+data);

					if (uhyve_close->fd > 2)
						uhyve_close->ret = close(uhyve_close->fd);
					else
						uhyve_close->ret = 0;
					break;
				}

			case UHYVE_PORT_LSEEK: {
					unsigned data = *((unsigned*)((size_t)run+run->io.data_offset));
					uhyve_lseek_t* uhyve_lseek = (uhyve_lseek_t*) (guest_mem+data);

					uhyve_lseek->offset = lseek(uhyve_lseek->fd, uhyve_lseek->offset, uhyve_lseek->whence);
					break;
				}
			default:
				err(1, "KVM: unhandled KVM_EXIT_IO at port 0x%x, direction %d\n", run->io.port, run->io.direction);
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
			err(1, "KVM: receive shutdown command\n");
			break;

		case KVM_EXIT_DEBUG:
			print_registers();
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
	struct kvm_mp_state mp_state = { KVM_MP_STATE_RUNNABLE };
	struct kvm_regs regs = {
		.rip = elf_entry,	// entry point to HermitCore
		.rflags = 0x2,		// POR value required by x86 architecture
	};

	vcpu_fds[cpuid] = vcpufd = kvm_ioctl(vmfd, KVM_CREATE_VCPU, cpuid);

	/* Map the shared kvm_run structure and following data. */
	size_t mmap_size = (size_t) kvm_ioctl(kvm, KVM_GET_VCPU_MMAP_SIZE, NULL);

	if (mmap_size < sizeof(*run))
		err(1, "KVM: invalid VCPU_MMAP_SIZE: %zd", mmap_size);

	run = mmap(NULL, mmap_size, PROT_READ | PROT_WRITE, MAP_SHARED, vcpufd, 0);
	if (run == MAP_FAILED)
		err(1, "KVM: VCPU mmap failed");

	setup_cpuid(kvm, vcpufd);

	if (restart) {
		char fname[MAX_FNAME];
		struct kvm_sregs sregs;
		struct kvm_fpu fpu;
		struct {
			struct kvm_msrs info;
			struct kvm_msr_entry entries[MAX_MSR_ENTRIES];
		} msr_data;
		struct kvm_lapic_state lapic;
		struct kvm_xsave xsave;
		struct kvm_xcrs xcrs;
		struct kvm_vcpu_events events;

		snprintf(fname, MAX_FNAME, "checkpoint/chk%u_core%u.dat", no_checkpoint, cpuid);

		FILE* f = fopen(fname, "r");
		if (f == NULL)
			err(1, "fopen: unable to open file");

		if (fread(&sregs, sizeof(sregs), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&regs, sizeof(regs), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&fpu, sizeof(fpu), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&msr_data, sizeof(msr_data), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&lapic, sizeof(lapic), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&xsave, sizeof(xsave), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&xcrs, sizeof(xcrs), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&events, sizeof(events), 1, f) != 1)
			err(1, "fread failed\n");
		if (fread(&mp_state, sizeof(mp_state), 1, f) != 1)
			err(1, "fread failed\n");

		fclose(f);

		kvm_ioctl(vcpufd, KVM_SET_SREGS, &sregs);
		kvm_ioctl(vcpufd, KVM_SET_REGS, &regs);
		kvm_ioctl(vcpufd, KVM_SET_MSRS, &msr_data);
		kvm_ioctl(vcpufd, KVM_SET_XCRS, &xcrs);
		kvm_ioctl(vcpufd, KVM_SET_MP_STATE, &mp_state);
		kvm_ioctl(vcpufd, KVM_SET_LAPIC, &lapic);
		kvm_ioctl(vcpufd, KVM_SET_FPU, &fpu);
		kvm_ioctl(vcpufd, KVM_SET_XSAVE, &xsave);
		kvm_ioctl(vcpufd, KVM_SET_VCPU_EVENTS, &events);
	} else {
		// be sure that the multiprocessor is runable
		kvm_ioctl(vcpufd, KVM_SET_MP_STATE, &mp_state);

		/* Setup registers and memory. */
		setup_system(vcpufd, guest_mem, cpuid);
		kvm_ioctl(vcpufd, KVM_SET_REGS, &regs);

		// only one core is able to enter startup code
		// => the wait for the predecessor core
		while (*((volatile uint32_t*) (mboot + 0x20)) < cpuid)
			pthread_yield();
		*((volatile uint32_t*) (mboot + 0x30)) = cpuid;
	}

	return 0;
}

static void save_cpu_state(void)
{
	struct {
		struct kvm_msrs info;
		struct kvm_msr_entry entries[MAX_MSR_ENTRIES];
	} msr_data;
	struct kvm_msr_entry *msrs = msr_data.entries;
	struct kvm_regs regs;
	struct kvm_sregs sregs;
	struct kvm_fpu fpu;
	struct kvm_lapic_state lapic;
	struct kvm_xsave xsave;
	struct kvm_xcrs xcrs;
	struct kvm_vcpu_events events;
	struct kvm_mp_state mp_state;
	char fname[MAX_FNAME];
	int n = 0;

	/* define the list of required MSRs */
	msrs[n++].index = MSR_IA32_APICBASE;
	msrs[n++].index = MSR_IA32_SYSENTER_CS;
	msrs[n++].index = MSR_IA32_SYSENTER_ESP;
	msrs[n++].index = MSR_IA32_SYSENTER_EIP;
	msrs[n++].index = MSR_IA32_CR_PAT;
	msrs[n++].index = MSR_IA32_MISC_ENABLE;
	msrs[n++].index = MSR_IA32_TSC;
	msrs[n++].index = MSR_CSTAR;
	msrs[n++].index = MSR_STAR;
	msrs[n++].index = MSR_EFER;
	msrs[n++].index = MSR_LSTAR;
	msrs[n++].index = MSR_GS_BASE;
	msrs[n++].index = MSR_FS_BASE;
	msrs[n++].index = MSR_KERNEL_GS_BASE;
	//msrs[n++].index = MSR_IA32_FEATURE_CONTROL;
	msr_data.info.nmsrs = n;

	kvm_ioctl(vcpufd, KVM_GET_SREGS, &sregs);
	kvm_ioctl(vcpufd, KVM_GET_REGS, &regs);
	kvm_ioctl(vcpufd, KVM_GET_MSRS, &msr_data);
	kvm_ioctl(vcpufd, KVM_GET_XCRS, &xcrs);
	kvm_ioctl(vcpufd, KVM_GET_LAPIC, &lapic);
	kvm_ioctl(vcpufd, KVM_GET_FPU, &fpu);
	kvm_ioctl(vcpufd, KVM_GET_XSAVE, &xsave);
	kvm_ioctl(vcpufd, KVM_GET_VCPU_EVENTS, &events);
	kvm_ioctl(vcpufd, KVM_GET_MP_STATE, &mp_state);

	snprintf(fname, MAX_FNAME, "checkpoint/chk%u_core%u.dat", no_checkpoint, cpuid);

	FILE* f = fopen(fname, "w");
	if (f == NULL) {
		err(1, "fopen: unable to open file\n");
	}

	if (fwrite(&sregs, sizeof(sregs), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&regs, sizeof(regs), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&fpu, sizeof(fpu), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&msr_data, sizeof(msr_data), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&lapic, sizeof(lapic), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&xsave, sizeof(xsave), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&xcrs, sizeof(xcrs), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&events, sizeof(events), 1, f) != 1)
		err(1, "fwrite failed\n");
	if (fwrite(&mp_state, sizeof(mp_state), 1, f) != 1)
		err(1, "fwrite failed\n");

	fclose(f);
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
	char* v = getenv("HERMIT_VERBOSE");
	if (v && (strcmp(v, "0") != 0))
		verbose = true;

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

	uint64_t identity_base = 0xfffbc000;
	if (ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_SYNC_MMU) > 0) {
		/* Allows up to 16M BIOSes. */
		identity_base = 0xfeffc000;

		kvm_ioctl(vmfd, KVM_SET_IDENTITY_MAP_ADDR, &identity_base);
	}
	kvm_ioctl(vmfd, KVM_SET_TSS_ADDR, identity_base + 0x1000);

	/*
	 * Allocate page-aligned guest memory.
	 *
	 * TODO: support of huge pages
	 */
	if (guest_size < KVM_32BIT_GAP_START) {
		guest_mem = mmap(NULL, guest_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
		if (guest_mem == MAP_FAILED)
			err(1, "mmap failed");
	} else {
		guest_size += + KVM_32BIT_GAP_SIZE;
		guest_mem = mmap(NULL, guest_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
		if (guest_mem == MAP_FAILED)
			err(1, "mmap failed");

		/*
		 * We mprotect the gap PROT_NONE so that if we accidently write to it, we will know.
		 */
		mprotect(guest_mem + KVM_32BIT_GAP_START, KVM_32BIT_GAP_SIZE, PROT_NONE);
	}

	const char* merge = getenv("HERMIT_MERGEABLE");
	if (merge && (strcmp(merge, "0") != 0)) {
		/*
		 * The KSM feature is intended for applications that generate
		 * many instances of the same data (e.g., virtualization systems
		 * such as KVM). It can consume a lot of processing power!
		 */
		madvise(guest_mem, guest_size, MADV_MERGEABLE);
		if (verbose)
			fprintf(stderr, "VM uses KSN feature \"mergeable\" to reduce the memory footprint.\n");
	}

	struct kvm_userspace_memory_region kvm_region = {
		.slot = 0,
		.guest_phys_addr = GUEST_OFFSET,
		.memory_size = guest_size,
		.userspace_addr = (uint64_t) guest_mem,
#ifdef USE_DIRTY_LOG
		.flags = KVM_MEM_LOG_DIRTY_PAGES,
#else
		.flags = 0,
#endif
	};

	if (guest_size <= KVM_32BIT_GAP_START - GUEST_OFFSET) {
		kvm_ioctl(vmfd, KVM_SET_USER_MEMORY_REGION, &kvm_region);
	} else {
		kvm_region.memory_size = KVM_32BIT_GAP_START - GUEST_OFFSET;
		kvm_ioctl(vmfd, KVM_SET_USER_MEMORY_REGION, &kvm_region);

		kvm_region.slot = 1;
		kvm_region.guest_phys_addr = KVM_32BIT_GAP_START+KVM_32BIT_GAP_SIZE;
		kvm_region.memory_size = guest_size - KVM_32BIT_GAP_SIZE - KVM_32BIT_GAP_START + GUEST_OFFSET;
		kvm_ioctl(vmfd, KVM_SET_USER_MEMORY_REGION, &kvm_region);
	}

	kvm_ioctl(vmfd, KVM_CREATE_IRQCHIP, NULL);

#ifdef KVM_CAP_X2APIC_API
	// enable x2APIC support
	struct kvm_enable_cap cap = {
		.cap = KVM_CAP_X2APIC_API,
		.flags = 0,
		.args[0] = KVM_X2APIC_API_USE_32BIT_IDS|KVM_X2APIC_API_DISABLE_BROADCAST_QUIRK,
	};
	kvm_ioctl(vmfd, KVM_ENABLE_CAP, &cap);
#endif

	// try to detect KVM extensions
	cap_tsc_deadline = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_TSC_DEADLINE_TIMER) <= 0 ? false : true;
	cap_irqchip = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_IRQCHIP) <= 0 ? false : true;
#ifdef KVM_CLOCK_TSC_STABLE
	cap_adjust_clock_stable = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_ADJUST_CLOCK) == KVM_CLOCK_TSC_STABLE ? true : false;
#endif

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
	return vcpu_init();
}

static void timer_handler(int signum)
{
	struct stat st = {0};
	const size_t flag = (!full_checkpoint && (no_checkpoint > 0)) ? PG_DIRTY : PG_ACCESSED;
	char fname[MAX_FNAME];
	struct timeval begin, end;

	if (verbose)
		gettimeofday(&begin, NULL);

	if (stat("checkpoint", &st) == -1)
		mkdir("checkpoint", 0700);

	for(size_t i = 0; i < ncores; i++)
		if (vcpu_threads[i] != pthread_self())
			pthread_kill(vcpu_threads[i], SIGRTMIN);

	pthread_barrier_wait(&barrier);

	save_cpu_state();

	snprintf(fname, MAX_FNAME, "checkpoint/chk%u_mem.dat", no_checkpoint);

	FILE* f = fopen(fname, "w");
	if (f == NULL) {
		err(1, "fopen: unable to open file");
	}

	/*struct kvm_irqchip irqchip = {};
	if (cap_irqchip)
		kvm_ioctl(vmfd, KVM_GET_IRQCHIP, &irqchip);
	else
		memset(&irqchip, 0x00, sizeof(irqchip));
	if (fwrite(&irqchip, sizeof(irqchip), 1, f) != 1)
		err(1, "fwrite failed");*/

	struct kvm_clock_data clock = {};
	kvm_ioctl(vmfd, KVM_GET_CLOCK, &clock);
	if (fwrite(&clock, sizeof(clock), 1, f) != 1)
		err(1, "fwrite failed");

#if 0
	if (fwrite(guest_mem, guest_size, 1, f) != 1)
		err(1, "fwrite failed");
#elif defined(USE_DIRTY_LOG)
	static struct kvm_dirty_log dlog = {
		.slot = 0,
		.dirty_bitmap = NULL
	};
	size_t dirty_log_size = (guest_size >> PAGE_BITS) / sizeof(size_t);

	// do we create our first checkpoint
	if (dlog.dirty_bitmap == NULL)
	{
		// besure that all paddings are zero
		memset(&dlog, 0x00, sizeof(dlog));

		dlog.dirty_bitmap = malloc(dirty_log_size * sizeof(size_t));
		if (dlog.dirty_bitmap == NULL)
			err(1, "malloc failed!\n");
	}
	memset(dlog.dirty_bitmap, 0x00, dirty_log_size * sizeof(size_t));

	dlog.slot = 0;
nextslot:
	kvm_ioctl(vmfd, KVM_GET_DIRTY_LOG, &dlog);

	for(size_t i=0; i<dirty_log_size; i++)
	{
		size_t value = ((size_t*) dlog.dirty_bitmap)[i];

		if (value)
		{
			for(size_t j=0; j<sizeof(size_t)*8; j++)
			{
				size_t test = 1ULL << j;

				if ((value & test) == test)
				{
					size_t addr = (i*sizeof(size_t)*8+j)*PAGE_SIZE;

					if (fwrite(&addr, sizeof(size_t), 1, f) != 1)
						err(1, "fwrite failed");
					if (fwrite((size_t*) (guest_mem + addr), PAGE_SIZE, 1, f) != 1)
						err(1, "fwrite failed");
				}
			}
		}
	}

	// do we have to check the second slot?
	if ((dlog.slot == 0) && (guest_size > KVM_32BIT_GAP_START - GUEST_OFFSET)) {
		dlog.slot = 1;
		memset(dlog.dirty_bitmap, 0x00, dirty_log_size * sizeof(size_t));
		goto nextslot;
	}
#else
	size_t* pml4 = (size_t*) (guest_mem+elf_entry+PAGE_SIZE);
	for(size_t i=0; i<(1 << PAGE_MAP_BITS); i++) {
		if ((pml4[i] & PG_PRESENT) != PG_PRESENT)
			continue;
		//printf("pml[%zd] 0x%zx\n", i, pml4[i]);
		size_t* pdpt = (size_t*) (guest_mem+(pml4[i] & PAGE_MASK));
		for(size_t j=0; j<(1 << PAGE_MAP_BITS); j++) {
			if ((pdpt[j] & PG_PRESENT) != PG_PRESENT)
				continue;
			//printf("\tpdpt[%zd] 0x%zx\n", j, pdpt[j]);
			size_t* pgd = (size_t*) (guest_mem+(pdpt[j] & PAGE_MASK));
			for(size_t k=0; k<(1 << PAGE_MAP_BITS); k++) {
				if ((pgd[k] & PG_PRESENT) != PG_PRESENT)
					continue;
				//printf("\t\tpgd[%zd] 0x%zx\n", k, pgd[k] & ~PG_XD);
				if ((pgd[k] & PG_PSE) != PG_PSE) {
					size_t* pgt = (size_t*) (guest_mem+(pgd[k] & PAGE_MASK));
					for(size_t l=0; l<(1 << PAGE_MAP_BITS); l++) {
						if ((pgt[l] & (PG_PRESENT|flag)) == (PG_PRESENT|flag)) {
							//printf("\t\t\t*pgt[%zd] 0x%zx, 4KB\n", l, pgt[l] & ~PG_XD);
							if (!full_checkpoint)
								pgt[l] = pgt[l] & ~(PG_DIRTY|PG_ACCESSED);
							size_t pgt_entry = pgt[l] & ~PG_PSE; // because PAT use the same bit as PSE
							if (fwrite(&pgt_entry, sizeof(size_t), 1, f) != 1)
								err(1, "fwrite failed");
							if (fwrite((size_t*) (guest_mem + (pgt[l] & PAGE_MASK)), (1UL << PAGE_BITS), 1, f) != 1)
								err(1, "fwrite failed");
						}
					}
				} else if ((pgd[k] & flag) == flag) {
					//printf("\t\t*pgd[%zd] 0x%zx, 2MB\n", k, pgd[k] & ~PG_XD);
					if (!full_checkpoint)
						pgd[k] = pgd[k] & ~(PG_DIRTY|PG_ACCESSED);
					if (fwrite(pgd+k, sizeof(size_t), 1, f) != 1)
						err(1, "fwrite failed");
					if (fwrite((size_t*) (guest_mem + (pgd[k] & PAGE_2M_MASK)), (1UL << PAGE_2M_BITS), 1, f) != 1)
						err(1, "fwrite failed");
				}
			}
		}
	}
#endif

	fclose(f);

	pthread_barrier_wait(&barrier);

	// update configuration file
	f = fopen("checkpoint/chk_config.txt", "w");
	if (f == NULL) {
		err(1, "fopen: unable to open file");
	}

	fprintf(f, "number of cores: %u\n", ncores);
	fprintf(f, "memory size: 0x%zx\n", guest_size);
	fprintf(f, "checkpoint number: %u\n", no_checkpoint);
	fprintf(f, "entry point: 0x%zx", elf_entry);
	if (full_checkpoint)
		fprintf(f, "full checkpoint: 1");
	else
		fprintf(f, "full checkpoint: 0");

	fclose(f);

	if (verbose) {
		gettimeofday(&end, NULL);
		size_t msec = (end.tv_sec - begin.tv_sec) * 1000;
		msec += (end.tv_usec - begin.tv_usec) / 1000;
		fprintf(stderr, "Create checkpoint %u in %zd ms\n", no_checkpoint, msec);
	}

	no_checkpoint++;
}

int uhyve_loop(void)
{
	const char* hermit_check = getenv("HERMIT_CHECKPOINT");
	int ts = 0;

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
