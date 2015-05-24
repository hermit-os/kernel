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
#include <hermit/processor.h>
#include <hermit/tasks.h>

extern void isrsyscall(void);

cpu_info_t cpu_info = { 0, 0, 0, 0};
extern uint32_t cpu_freq;

static void default_mb(void)
{
	asm volatile ("lock; addl $0,0(%%esp)" ::: "memory", "cc");
}

static void default_save_fpu_state(union fpu_state* state)
{
	asm volatile ("fnsave %0; fwait" : "=m"((*state).fsave) :: "memory");
}

static void default_restore_fpu_state(union fpu_state* state)
{
	asm volatile ("frstor %0" :: "m"(state->fsave));
}

static void default_fpu_init(union fpu_state* fpu)
{
	i387_fsave_t *fp = &fpu->fsave;

	memset(fp, 0x00, sizeof(i387_fsave_t));
	fp->cwd = 0xffff037fu;
	fp->swd = 0xffff0000u;
	fp->twd = 0xffffffffu;
	fp->fos = 0xffff0000u;
}

func_memory_barrier mb = default_mb;
func_memory_barrier rmb = default_mb;
func_memory_barrier wmb = default_mb;

static void mfence(void) { asm volatile("mfence" ::: "memory"); }
static void lfence(void) { asm volatile("lfence" ::: "memory"); }
static void sfence(void) { asm volatile("sfence" ::: "memory"); }
handle_fpu_state save_fpu_state = default_save_fpu_state;
handle_fpu_state restore_fpu_state = default_restore_fpu_state;
handle_fpu_state fpu_init = default_fpu_init;

static void save_fpu_state_fxsr(union fpu_state* state)
{
	asm volatile ("fxsave %0; fnclex" : "=m"((*state).fxsave) :: "memory");
}

static void restore_fpu_state_fxsr(union fpu_state* state)
{
	asm volatile ("fxrstor %0" :: "m"(state->fxsave));
}

static void fpu_init_fxsr(union fpu_state* fpu)
{
	i387_fxsave_t* fx = &fpu->fxsave;

	memset(fx, 0x00, sizeof(i387_fxsave_t));
	fx->cwd = 0x37f;
	if (BUILTIN_EXPECT(has_sse(), 1))
		fx->mxcsr = 0x1f80;
}

uint32_t detect_cpu_frequency(void)
{
	uint64_t start, end, diff;
	uint64_t ticks, old;

	if (BUILTIN_EXPECT(cpu_freq > 0, 0))
		return cpu_freq;

	old = get_clock_tick();

	/* wait for the next time slice */
	while((ticks = get_clock_tick()) - old == 0)
		HALT;

	rmb();
	start = rdtsc();
	/* wait a second to determine the frequency */
	while(get_clock_tick() - ticks < TIMER_FREQ)
		HALT;
	rmb();
	end = rdtsc();

	diff = end > start ? end - start : start - end;
	cpu_freq = (uint32_t) (diff / (uint64_t) 1000000);

	return cpu_freq;
}

int cpu_detection(void) {
	uint32_t a=0, b=0, c=0, d=0;
	uint32_t family, model, stepping;
	size_t cr4;
	uint8_t first_time = 0;

	if (!cpu_info.feature1) {
		first_time = 1;
		cpuid(1, &a, &b, &cpu_info.feature2, &cpu_info.feature1);

		family   = (a & 0x00000F00) >> 8;
		model    = (a & 0x000000F0) >> 4;
		stepping =  a & 0x0000000F;
		if ((family == 6) && (model < 3) && (stepping < 3))
			cpu_info.feature1 &= ~CPU_FEATURE_SEP;

		cpuid(0x80000001, &a, &b, &c, &cpu_info.feature3);
		cpuid(0x80000008, &cpu_info.addr_width, &b, &c, &d);
	}

	if (first_time) {
		kprintf("Paging features: %s%s%s%s%s%s%s%s\n",
				(cpu_info.feature1 & CPU_FEATUE_PSE) ? "PSE (2/4Mb) " : "",
				(cpu_info.feature1 & CPU_FEATURE_PAE) ? "PAE " : "",
				(cpu_info.feature1 & CPU_FEATURE_PGE) ? "PGE " : "",
				(cpu_info.feature1 & CPU_FEATURE_PAT) ? "PAT " : "",
				(cpu_info.feature1 & CPU_FEATURE_PSE36) ? "PSE36 " : "",
				(cpu_info.feature3 & CPU_FEATURE_NX) ? "NX " : "",
				(cpu_info.feature3 & CPU_FEATURE_1GBHP) ? "PSE (1Gb) " : "",
				(cpu_info.feature3 & CPU_FEATURE_LM) ? "LM" : "");

		kprintf("Physical adress-width: %u bits\n", cpu_info.addr_width & 0xff);
		kprintf("Linear adress-width: %u bits\n", (cpu_info.addr_width >> 8) & 0xff);
		kprintf("Sysenter instruction: %s\n", (cpu_info.feature1 & CPU_FEATURE_SEP) ? "available" : "unavailable");
		kprintf("Syscall instruction: %s\n", (cpu_info.feature3 & CPU_FEATURE_SYSCALL) ? "available" : "unavailable");
	}

	cr4 = read_cr4();
	if (has_fxsr())
		cr4 |= CR4_OSFXSR;		// set the OSFXSR bit
	if (has_sse())
		cr4 |= CR4_OSXMMEXCPT;	// set the OSXMMEXCPT bit
	if (has_pge())
		cr4 |= CR4_PGE;
	write_cr4(cr4);

	if (cpu_info.feature3 & CPU_FEATURE_SYSCALL) {
		wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_LMA | EFER_SCE);
		wrmsr(MSR_STAR, (0x1BULL << 48) | (0x08ULL << 32));
		wrmsr(MSR_LSTAR, (size_t) &isrsyscall);
		wrmsr(MSR_SYSCALL_MASK, 0); // we didn't clear RFLAGS during an interrupt
	} else kputs("Processor doesn't support syscalls\n");

	if (has_nx())
		wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_NXE);

	if (first_time && has_sse())
		wmb = sfence;

	if (first_time && has_sse2()) {
		rmb = lfence;
		mb = mfence;
	}

	if (first_time && has_avx())
		kprintf("The CPU owns the Advanced Vector Extensions (AVX). However, HermitCore doesn't support AVX!\n");

	if (has_fpu()) {
		if (first_time)
			kputs("Found and initialized FPU!\n");
		asm volatile ("fninit");
	}

	if (first_time && has_fxsr()) {
		save_fpu_state = save_fpu_state_fxsr;
		restore_fpu_state = restore_fpu_state_fxsr;
		fpu_init = fpu_init_fxsr;
	}

	if (first_time && on_hypervisor()) {
		uint32_t c, d;
		char vendor_id[13];

		kprintf("HermitCore is running on a hypervisor!\n");

		cpuid(0x40000000, &a, &b, &c, &d);
		memcpy(vendor_id, &b, 4);
		memcpy(vendor_id + 4, &c, 4);
		memcpy(vendor_id + 8, &d, 4);
		vendor_id[12] = '\0';

		kprintf("Hypervisor Vendor Id: %s\n", vendor_id);
		kprintf("Maximum input value for hypervisor: 0x%x\n", a);
	}

	return 0;
}

uint32_t get_cpu_frequency(void)
{	
	if (cpu_freq > 0)
		return cpu_freq;

	return detect_cpu_frequency();
}

