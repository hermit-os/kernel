/*
 * Copyright (c) 2018, Stefan Lankes, RWTH Aachen University
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

#ifdef __x86_64__
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
#include <asm/msr-index.h>
#include <asm/mman.h>

#include "uhyve.h"
#include "uhyve-x86_64.h"
#include "uhyve-syscalls.h"
#include "uhyve-net.h"
#include "proxy.h"

// define this macro to create checkpoints with KVM's dirty log
//#define USE_DIRTY_LOG

#define MAX_FNAME       256
#define MAX_MSR_ENTRIES 25

#define GUEST_OFFSET		0x0
#define CPUID_FUNC_PERFMON	0x0A
#define GUEST_PAGE_SIZE		0x200000   /* 2 MB pages in guest */

#define KVM_32BIT_MAX_MEM_SIZE  (1ULL << 32)
#define KVM_32BIT_GAP_SIZE      (768 << 20)
#define KVM_32BIT_GAP_START     (KVM_32BIT_MAX_MEM_SIZE - KVM_32BIT_GAP_SIZE)

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
#define PAGE_MASK			(((~0UL) << PAGE_BITS) & ~PG_XD)
#define PAGE_2M_MASK	(((~0UL) << PAGE_2M_BITS) & ~PG_XD)
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

#define IOAPIC_DEFAULT_BASE	0xfec00000
#define APIC_DEFAULT_BASE	0xfee00000

static bool cap_tsc_deadline = false;
static bool cap_irqchip = false;
static bool cap_adjust_clock_stable = false;
static bool cap_irqfd = false;
static bool cap_vapic = false;

extern size_t guest_size;
extern pthread_barrier_t barrier;
extern pthread_t* vcpu_threads;
extern uint64_t elf_entry;
extern uint8_t* klog;
extern bool verbose;
extern bool full_checkpoint;
extern uint32_t no_checkpoint;
extern uint32_t ncores;
extern uint8_t* guest_mem;
extern size_t guest_size;
extern int kvm, vmfd, netfd, efd;
extern uint8_t* mboot;
extern __thread struct kvm_run *run;
extern __thread int vcpufd;
extern __thread uint32_t cpuid;

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

void print_registers(void)
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

void init_cpu_state(uint64_t elf_entry)
{
	struct kvm_regs regs = {
		.rip = elf_entry,	// entry point to HermitCore
		.rflags = 0x2,		// POR value required by x86 architecture
	};
	struct kvm_mp_state mp_state = { KVM_MP_STATE_RUNNABLE };
	struct {
		struct kvm_msrs info;
		struct kvm_msr_entry entries[MAX_MSR_ENTRIES];
	} msr_data;
	struct kvm_msr_entry *msrs = msr_data.entries;

	run->apic_base = APIC_DEFAULT_BASE;
        setup_cpuid(kvm, vcpufd);

	// be sure that the multiprocessor is runable
	kvm_ioctl(vcpufd, KVM_SET_MP_STATE, &mp_state);

	// enable fast string operations
	msrs[0].index = MSR_IA32_MISC_ENABLE;
	msrs[0].data = 1;
	msr_data.info.nmsrs = 1;
	kvm_ioctl(vcpufd, KVM_SET_MSRS, &msr_data);

	/* Setup registers and memory. */
	setup_system(vcpufd, guest_mem, cpuid);
	kvm_ioctl(vcpufd, KVM_SET_REGS, &regs);

	// only one core is able to enter startup code
	// => the wait for the predecessor core
	while (*((volatile uint32_t*) (mboot + 0x20)) < cpuid)
		pthread_yield();
	*((volatile uint32_t*) (mboot + 0x30)) = cpuid;
}

void restore_cpu_state(void)
{
	struct kvm_regs regs;
	struct kvm_mp_state mp_state = { KVM_MP_STATE_RUNNABLE };
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

	run->apic_base = APIC_DEFAULT_BASE;
        setup_cpuid(kvm, vcpufd);

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

}

void save_cpu_state(void)
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

void timer_handler(int signum)
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
	fprintf(f, "entry point: 0x%zx\n", elf_entry);
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

int load_checkpoint(uint8_t* mem, char* path)
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

void init_kvm_arch(void)
{
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
		guest_size += KVM_32BIT_GAP_SIZE;
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

	const char* hugepage = getenv("HERMIT_HUGEPAGE");
	if (merge && (strcmp(merge, "0") != 0)) {
		madvise(guest_mem, guest_size, MADV_HUGEPAGE);
		if (verbose)
			fprintf(stderr, "VM uses huge pages to improve the performance.\n");
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

	// initialited IOAPIC with HermitCore's default settings
	struct kvm_irqchip chip;
	chip.chip_id = KVM_IRQCHIP_IOAPIC;
	kvm_ioctl(vmfd, KVM_GET_IRQCHIP, &chip);
	for(int i=0; i<KVM_IOAPIC_NUM_PINS; i++) {
		chip.chip.ioapic.redirtbl[i].fields.vector = 0x20+i;
		chip.chip.ioapic.redirtbl[i].fields.delivery_mode = 0;
		chip.chip.ioapic.redirtbl[i].fields.dest_mode = 0;
		chip.chip.ioapic.redirtbl[i].fields.delivery_status = 0;
		chip.chip.ioapic.redirtbl[i].fields.polarity = 0;
		chip.chip.ioapic.redirtbl[i].fields.remote_irr = 0;
		chip.chip.ioapic.redirtbl[i].fields.trig_mode = 0;
		chip.chip.ioapic.redirtbl[i].fields.mask = i != 2 ? 0 : 1;
		chip.chip.ioapic.redirtbl[i].fields.dest_id = 0;
	}
	kvm_ioctl(vmfd, KVM_SET_IRQCHIP, &chip);

	// try to detect KVM extensions
	cap_tsc_deadline = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_TSC_DEADLINE_TIMER) <= 0 ? false : true;
	cap_irqchip = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_IRQCHIP) <= 0 ? false : true;
#ifdef KVM_CLOCK_TSC_STABLE
	cap_adjust_clock_stable = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_ADJUST_CLOCK) == KVM_CLOCK_TSC_STABLE ? true : false;
#endif
	cap_irqfd = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_IRQFD) <= 0 ? false : true;
	if (!cap_irqfd)
		err(1, "the support of KVM_CAP_IRQFD is curently required");
	// TODO: add VAPIC support
	cap_vapic = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_VAPIC) <= 0 ? false : true;
	//if (cap_vapic)
	//	printf("System supports vapic\n");
}

int load_kernel(uint8_t* mem, char* path)
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
		fprintf(stderr, "Invalid HermitCore file!\n");
		ret = -1;
		goto out;
	}

	elf_entry = hdr.e_entry;

	buflen = hdr.e_phentsize * hdr.e_phnum;
	phdr = malloc(buflen);
	if (!phdr) {
		fprintf(stderr, "Not enough memory\n");
		ret = -1;
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


			char* str = getenv("HERMIT_IP");
			if (str) {
				uint32_t ip[4];

				sscanf(str, "%u.%u.%u.%u",	ip+0, ip+1, ip+2, ip+3);
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB0)) = (uint8_t) ip[0];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB1)) = (uint8_t) ip[1];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB2)) = (uint8_t) ip[2];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB3)) = (uint8_t) ip[3];
			}

			str = getenv("HERMIT_GATEWAY");
			if (str) {
				uint32_t ip[4];

				sscanf(str, "%u.%u.%u.%u",	ip+0, ip+1, ip+2, ip+3);
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB4)) = (uint8_t) ip[0];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB5)) = (uint8_t) ip[1];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB6)) = (uint8_t) ip[2];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB7)) = (uint8_t) ip[3];
			}
			str = getenv("HERMIT_MASK");
			if (str) {
				uint32_t ip[4];

				sscanf(str, "%u.%u.%u.%u",	ip+0, ip+1, ip+2, ip+3);
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB8)) = (uint8_t) ip[0];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xB9)) = (uint8_t) ip[1];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xBA)) = (uint8_t) ip[2];
				*((uint8_t*) (mem+paddr-GUEST_OFFSET + 0xBB)) = (uint8_t) ip[3];
			}

			*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0xbc)) = guest_mem;
		}
		*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0x38)) += memsz; // total kernel size
	}

	ret = 0;

out:
	if (phdr)
		free(phdr);

	close(fd);

	return ret;
}
#endif
