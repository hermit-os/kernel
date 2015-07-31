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

/*
 *  * Note that linker symbols are not variables, they have no memory allocated for
 *   * maintaining a value, rather their address is their value.
 *    */
extern const void percore_start;
extern const void percore_end0;
extern const void percore_end;

extern void isrsyscall(void);

cpu_info_t cpu_info = { 0, 0, 0, 0, 0};
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
	asm volatile ("fxsave %0; fnclex" : "=m"(state->fxsave) :: "memory");
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

static void save_fpu_state_xsave(union fpu_state* state)
{
	asm volatile ("xsaveq %0" : "=m"(state->xsave) : "a"(-1), "d"(-1) : "memory");
}

static void restore_fpu_state_xsave(union fpu_state* state)
{
	asm volatile ("xrstorq %0" :: "m"(state->xsave), "a"(-1), "d"(-1));
}

static void fpu_init_xsave(union fpu_state* fpu)
{
	xsave_t* xs = &fpu->xsave;

	memset(xs, 0x00, sizeof(xsave_t));
	xs->fxsave.cwd = 0x37f;
	xs->fxsave.mxcsr = 0x1f80;
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
	uint64_t xcr0;
	uint32_t a=0, b=0, c=0, d=0, level = 0;
	uint32_t family, model, stepping;
	size_t cr4;
	uint8_t first_time = 0;

	if (!cpu_info.feature1) {
		first_time = 1;

		cpuid(0, &level, &b, &c, &d);
		kprintf("cpuid level %d\n", level);

		a = b = c = d = 0;
		cpuid(1, &a, &b, &cpu_info.feature2, &cpu_info.feature1);

		family   = (a & 0x00000F00) >> 8;
		model    = (a & 0x000000F0) >> 4;
		stepping =  a & 0x0000000F;
		if ((family == 6) && (model < 3) && (stepping < 3))
			cpu_info.feature1 &= ~CPU_FEATURE_SEP;

		cpuid(0x80000001, &a, &b, &c, &cpu_info.feature3);
		cpuid(0x80000008, &cpu_info.addr_width, &b, &c, &d);

		/* Additional Intel-defined flags: level 0x00000007 */
        	if (level >= 0x00000007) {
			a = b = c = d = 0;
			cpuid(7, &a, &cpu_info.feature4, &c, &d);
		}
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
		cr4 |= CR4_OSFXSR;	// set the OSFXSR bit
	if (has_sse())
		cr4 |= CR4_OSXMMEXCPT;	// set the OSXMMEXCPT bit
	if (has_xsave())
		cr4 |= CR4_OSXSAVE;
	if (has_pge())
		cr4 |= CR4_PGE;
	cr4 &= ~CR4_TSD;		// => every privilege level is able to use rdtsc
	write_cr4(cr4);

	if (has_xsave())
	{
		xcr0 = xgetbv(0);
		if (has_fpu())
			xcr0 |= 0x1;
		if (has_sse())
			xcr0 |= 0x2;
		if (has_avx())
			xcr0 |= 0x4;
		xsetbv(0, xcr0);

		kprintf("Set XCR0 to 0x%llx\n", xgetbv(0));
	}

	if (cpu_info.feature3 & CPU_FEATURE_SYSCALL) {
		wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_LMA | EFER_LME | EFER_NXE | EFER_SCE);
		wrmsr(MSR_STAR, (0x1BULL << 48) | (0x08ULL << 32));
		wrmsr(MSR_LSTAR, (size_t) &isrsyscall);
		//  clear IF flag during an interrupt
		wrmsr(MSR_SYSCALL_MASK, EFLAGS_TF|EFLAGS_DF|EFLAGS_IF|EFLAGS_AC|EFLAGS_NT);
	} else kputs("Processor doesn't support syscalls\n");

	if (has_nx())
		wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_NXE);

	wrmsr(MSR_FS_BASE, 0);
#if MAX_CORES > 1
	wrmsr(MSR_GS_BASE, apic_cpu_id() * ((size_t) &percore_end0 - (size_t) &percore_start));
#else
	wrmsr(MSR_GS_BASE, 0);
#endif
	wrmsr(MSR_KERNEL_GS_BASE, 0);

	kprintf("Core %d set per_core offset to 0x%x\n", apic_cpu_id(), rdmsr(MSR_GS_BASE));

#if MAX_CORES > 1
	/* set core id to apic_cpu_id */
	set_per_core(__core_id, apic_cpu_id());
#endif

	if (first_time && has_sse())
		wmb = sfence;

	if (first_time && has_sse2()) {
		rmb = lfence;
		mb = mfence;
	}

	if (has_fpu()) {
		if (first_time)
			kputs("Found and initialized FPU!\n");
		asm volatile ("fninit");
	}

	if (first_time) {
		// reload feature list because we enabled OSXSAVE
		a = b = c = d = 0;
                cpuid(1, &a, &b, &cpu_info.feature2, &cpu_info.feature1);

		kprintf("CPU features: %s%s%s%s%s%s%s%s%s%s%s%s%s%s%s\n",
			has_sse() ? "SSE " : "",
			has_sse2() ? "SSE2 " : "",
			has_sse3() ? "SSE3 " : "",
			has_sse4_1() ? "SSE4.1 " : "",
			has_sse4_2() ? "SSE4.2 " : "",
			has_avx() ? "AVX " : "",
			has_avx2() ? "AVX2 " : "",
			has_fma() ? "FMA " : "",
			has_movbe() ? "MOVBE " : "",
			has_x2apic() ? "X2APIC " : "",
			has_fpu() ? "FPU " : "",
			has_fxsr() ? "FXSR " : "",
			has_xsave() ? "XSAVE " : "",
			has_osxsave() ? "OSXSAVE " : "",
			has_rdtscp() ? "RDTSCP " : "");
	}

	if (first_time && has_osxsave()) {
		a = b = d = 0;
		c = 2;
		cpuid(0xd, &a, &b, &c, &d);
		kprintf("Ext_Save_Area_2: offset %d, size %d\n", b, a);

		a = b = d = 0;
		c = 3;
		cpuid(0xd, &a, &b, &c, &d);
		kprintf("Ext_Save_Area_3: offset %d, size %d\n", b, a);

		a = b = d = 0;
		c = 4;
		cpuid(0xd, &a, &b, &c, &d);
		kprintf("Ext_Save_Area_4: offset %d, size %d\n", b, a);

		save_fpu_state = save_fpu_state_xsave;
		restore_fpu_state = restore_fpu_state_xsave;
		fpu_init = fpu_init_xsave;
	} else if (first_time && has_fxsr()) {
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

	if (first_time) {
		kprintf("CR0 0x%llx, CR4 0x%llx\n", read_cr0(), read_cr4());
		kprintf("size of xsave_t: %d\n", sizeof(xsave_t));
	}

	return 0;
}

uint32_t get_cpu_frequency(void)
{
	if (cpu_freq > 0)
		return cpu_freq;

	return detect_cpu_frequency();
}

void udelay(uint32_t usecs)
{
	uint64_t diff, end, start = rdtsc();
	uint64_t deadline = get_cpu_frequency() * usecs;

	do {
		mb();
		end = rdtsc();
		diff = end > start ? end - start : start - end;
		if ((diff < deadline) && (deadline - diff > 50000))
			check_workqueues();
	} while(diff < deadline);
}
