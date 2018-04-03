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

#ifdef __aarch64__

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
#include "proxy.h"

#define GUEST_OFFSET		0x0

#define GIC_SPI_IRQ_BASE	32
#define GICD_BASE		(1ULL << 39)
#define GICC_BASE		(GICD_BASE + GICD_SIZE)
#define GIC_SIZE		(GICD_SIZE + GICC_SIZE)
#define GICD_SIZE		0x10000ULL
#define GICC_SIZE		0x20000ULL

#define KVM_GAP_SIZE		(GIC_SIZE)
#define KVM_GAP_START		GICD_BASE

#define PAGE_SIZE		0x1000

#ifndef offsetof
#define offsetof(TYPE, MEMBER)		((size_t) &((TYPE *)0)->MEMBER)
#endif
#define ARM64_CORE_REG(x)		(KVM_REG_ARM64 | KVM_REG_SIZE_U64 |\
					 KVM_REG_ARM_CORE | KVM_REG_ARM_CORE_REG(x))

static bool cap_irqfd = false;
static bool cap_read_only = false;
static int gic_fd = -1;

extern size_t guest_size;
extern uint64_t elf_entry;
extern uint8_t* klog;
extern bool verbose;
extern uint8_t* guest_mem;
extern size_t guest_size;
extern int kvm, vmfd, netfd, efd;
extern uint8_t* mboot;
extern __thread struct kvm_run *run;
extern __thread int vcpufd;
extern __thread uint32_t cpuid;

void print_registers(void)
{
	struct kvm_one_reg reg;
	uint64_t data;

	fprintf(stderr, "\n Dump state of CPU %d\n\n", cpuid);
	fprintf(stderr, " Registers\n");
	fprintf(stderr, " =========\n");

	reg.addr = (uint64_t)&data;
	reg.id = ARM64_CORE_REG(regs.pc);
	kvm_ioctl(vcpufd, KVM_GET_ONE_REG, &reg);
	fprintf(stderr, " PC:     0x%016lx\n", data);

	reg.id = ARM64_CORE_REG(regs.pstate);
	kvm_ioctl(vcpufd, KVM_GET_ONE_REG, &reg);
	fprintf(stderr, " PSTATE: 0x%016lx\n", data);

	reg.id = ARM64_CORE_REG(sp_el1);
	kvm_ioctl(vcpufd, KVM_GET_ONE_REG, &reg);
	fprintf(stderr, " SP_EL1: 0x%016lx\n", data);

	reg.id = ARM64_CORE_REG(regs.regs[30]);
	kvm_ioctl(vcpufd, KVM_GET_ONE_REG, &reg);
	fprintf(stderr, " LR:     0x%016lx\n", data);

	for(int i=0; i<=29; i+=2)
	{
		reg.id = ARM64_CORE_REG(regs.regs[i]);
		kvm_ioctl(vcpufd, KVM_GET_ONE_REG, &reg);
		fprintf(stderr, " X%d:\t 0x%016lx\t", i, data);

		reg.id = ARM64_CORE_REG(regs.regs[i+1]);
		kvm_ioctl(vcpufd, KVM_GET_ONE_REG, &reg);
		fprintf(stderr, " X%d:\t0x%016lx\n", i+1, data);
	}
}

void timer_handler(int signum)
{
	err(1, "Checkpointing is currently not supported!");
}

void restore_cpu_state(void)
{
	err(1, "Checkpointing is currently not supported!");
}

void save_cpu_state(void)
{
	err(1, "Checkpointing is currently not supported!");
}

int load_checkpoint(uint8_t* mem, char* path)
{
	err(1, "Checkpointing is currently not supported!");
}

void init_cpu_state(uint64_t elf_entry)
{
	struct kvm_vcpu_init vcpu_init = {
                .features = 0,
        };
        struct kvm_vcpu_init preferred_init;

	if (!ioctl(vmfd, KVM_ARM_PREFERRED_TARGET, &preferred_init)) {
		if ((preferred_init.target == KVM_ARM_TARGET_CORTEX_A57) ||
		    (preferred_init.target == KVM_ARM_TARGET_CORTEX_A53)) {
			vcpu_init.target = preferred_init.target;
		} else {
			vcpu_init.target = KVM_ARM_TARGET_GENERIC_V8;
		}
        } else {
                vcpu_init.target = KVM_ARM_TARGET_GENERIC_V8;
        }

        kvm_ioctl(vcpufd, KVM_ARM_VCPU_INIT, &vcpu_init);

	// be sure that the multiprocessor is runable
	struct kvm_mp_state mp_state = { KVM_MP_STATE_RUNNABLE };
	kvm_ioctl(vcpufd, KVM_SET_MP_STATE, &mp_state);

	struct kvm_one_reg reg;
	uint64_t data;

	/* pstate = all interrupts masked */
	data	= PSR_D_BIT | PSR_A_BIT | PSR_I_BIT | PSR_F_BIT | PSR_MODE_EL1h;
	reg.id	= ARM64_CORE_REG(regs.pstate);
	reg.addr = (uint64_t)&data;
	kvm_ioctl(vcpufd, KVM_SET_ONE_REG, &reg);

#if 0
	/* x0...x3 = 0 */
	data    = 0;
        reg.id  = ARM64_CORE_REG(regs.regs[0]);
        kvm_ioctl(vcpufd, KVM_SET_ONE_REG, &reg);

	reg.id	= ARM64_CORE_REG(regs.regs[1]);
	kvm_ioctl(vcpufd, KVM_SET_ONE_REG, &reg);

	reg.id	= ARM64_CORE_REG(regs.regs[2]);
	kvm_ioctl(vcpufd, KVM_SET_ONE_REG, &reg);

	reg.id	= ARM64_CORE_REG(regs.regs[3]);
	kvm_ioctl(vcpufd, KVM_SET_ONE_REG, &reg);
#endif

	/* set start address */
	data	= elf_entry;
	reg.id	= ARM64_CORE_REG(regs.pc);
	kvm_ioctl(vcpufd, KVM_SET_ONE_REG, &reg);

	if (gic_fd > 0) {
		int lines = 1;
		uint32_t nr_irqs = lines * 32 + GIC_SPI_IRQ_BASE;
		struct kvm_device_attr nr_irqs_attr = {
			.group	= KVM_DEV_ARM_VGIC_GRP_NR_IRQS,
			.addr	= (uint64_t)&nr_irqs,
		};
		struct kvm_device_attr vgic_init_attr = {
			.group	= KVM_DEV_ARM_VGIC_GRP_CTRL,
			.attr	= KVM_DEV_ARM_VGIC_CTRL_INIT,
		};

		kvm_ioctl(gic_fd, KVM_SET_DEVICE_ATTR, &nr_irqs_attr);
		kvm_ioctl(gic_fd, KVM_SET_DEVICE_ATTR, &vgic_init_attr);
	}

	// only one core is able to enter startup code
	// => the wait for the predecessor core
	while (*((volatile uint32_t*) (mboot + 0x120)) < cpuid)
		pthread_yield();
	*((volatile uint32_t*) (mboot + 0x130)) = cpuid;
}

void init_kvm_arch(void)
{
	guest_mem = mmap(NULL, guest_size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	if (guest_mem == MAP_FAILED)
		err(1, "mmap failed");

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

	cap_read_only = kvm_ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_READONLY_MEM) <= 0 ? false : true;
	if (!cap_read_only)
		err(1, "the support of KVM_CAP_READONLY_MEM is curently required");

	struct kvm_userspace_memory_region kvm_region = {
		.slot = 0,
		.guest_phys_addr = 0,
		.memory_size = PAGE_SIZE,
		.userspace_addr = (uint64_t) guest_mem,
		.flags = KVM_MEM_READONLY,
	};
	kvm_ioctl(vmfd, KVM_SET_USER_MEMORY_REGION, &kvm_region);

	kvm_region = (struct kvm_userspace_memory_region) {
		.slot = 1,
		.guest_phys_addr = PAGE_SIZE,
		.memory_size = guest_size - PAGE_SIZE,
		.userspace_addr = (uint64_t) guest_mem + PAGE_SIZE,
 #ifdef USE_DIRTY_LOG
		.flags = KVM_MEM_LOG_DIRTY_PAGES,
 #else
 		.flags = 0,
 #endif
	};
	kvm_ioctl(vmfd, KVM_SET_USER_MEMORY_REGION, &kvm_region);

#if 0
	/* Create interrupt controller GICv2 */
	uint64_t cpu_if_addr = GICC_BASE;
	uint64_t dist_addr = GICD_BASE;
	struct kvm_device_attr cpu_if_attr = {
		.group	= KVM_DEV_ARM_VGIC_GRP_ADDR,
		.attr	= KVM_VGIC_V2_ADDR_TYPE_CPU,
		.addr	= (uint64_t)&cpu_if_addr,
	};
	struct kvm_create_device gic_device = {
		.flags	= 0,
		.type = KVM_DEV_TYPE_ARM_VGIC_V2,
	};
	struct kvm_device_attr dist_attr = {
		.group	= KVM_DEV_ARM_VGIC_GRP_ADDR,
		.attr	= KVM_VGIC_V2_ADDR_TYPE_DIST,
		.addr	= (uint64_t)&dist_addr,
	};
	kvm_ioctl(vmfd, KVM_CREATE_DEVICE, &gic_device);

	gic_fd = gic_device.fd;
	kvm_ioctl(gic_fd, KVM_SET_DEVICE_ATTR, &cpu_if_attr);
	kvm_ioctl(gic_fd, KVM_SET_DEVICE_ATTR, &dist_attr);
#else
	/* Create interrupt controller GICv2 */
	struct kvm_arm_device_addr gic_addr[] = {
		[0] = {
			.id = KVM_VGIC_V2_ADDR_TYPE_DIST |
			(KVM_ARM_DEVICE_VGIC_V2 << KVM_ARM_DEVICE_ID_SHIFT),
			.addr = GICD_BASE,
		},
		[1] = {
			.id = KVM_VGIC_V2_ADDR_TYPE_CPU |
			(KVM_ARM_DEVICE_VGIC_V2 << KVM_ARM_DEVICE_ID_SHIFT),
			.addr = GICC_BASE,
		}
	};

	kvm_ioctl(vmfd, KVM_CREATE_IRQCHIP, NULL);
	kvm_ioctl(vmfd, KVM_ARM_SET_DEVICE_ADDR, &gic_addr[0]);
	kvm_ioctl(vmfd, KVM_ARM_SET_DEVICE_ADDR, &gic_addr[1]);
#endif

	//fprintf(stderr, "Create gicd at 0x%llx\n", GICD_BASE);
	//fprintf(stderr, "Create gicc at 0x%llx\n", GICC_BASE);

	cap_irqfd = ioctl(vmfd, KVM_CHECK_EXTENSION, KVM_CAP_IRQFD) <= 0 ? false : true;
	if (!cap_irqfd)
            err(1, "the support of KVM_CAP_IRQFD is curently required");
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
	    || hdr.e_type != ET_EXEC || hdr.e_machine != EM_AARCH64) {
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

		//fprintf(stderr, "Kernel location 0x%zx, file size 0x%zx, memory size 0x%zx\n", paddr, filesz, memsz);

		ret = pread_in_full(fd, mem+paddr-GUEST_OFFSET, filesz, offset);
		if (ret < 0)
			goto out;
		if (!klog)
			klog = mem+paddr+0x1000-GUEST_OFFSET;
		if (!mboot)
			mboot = mem+paddr-GUEST_OFFSET;
		//fprintf(stderr, "mboot at %p, klog at %p\n", mboot, klog);

		if (first_load) {
			first_load = 0;

			// initialize kernel
			*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0x100)) = paddr; // physical start address
			*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0x108)) = guest_size - PAGE_SIZE;   // physical limit
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x110)) = get_cpufreq();
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x128)) = 1; // number of used cpus
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x130)) = 0; // cpuid
			*((uint32_t*) (mem+paddr-GUEST_OFFSET + 0x148)) = 1; // announce uhyve


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

			*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0xbc)) = (uint64_t) guest_mem;
		}
		*((uint64_t*) (mem+paddr-GUEST_OFFSET + 0x158)) += memsz; // total kernel size
	}

	ret = 0;

out:
	if (phdr)
		free(phdr);

	close(fd);

	return ret;
}
#endif
