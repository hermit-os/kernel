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
#include <hermit/logging.h>
#include <asm/multiboot.h>

/*
 * Note that linker symbols are not variables, they have no memory allocated for
 * maintaining a value, rather their address is their value.
 */
extern const void percore_start;
extern const void percore_end0;
extern const void percore_end;
extern void* Lpatch0;
extern void* Lpatch1;
extern void* Lpatch2;
extern atomic_int32_t current_boot_id;

extern void isrsyscall(void);

cpu_info_t cpu_info = { 0, 0, 0, 0, 0};
extern uint32_t cpu_freq;

uint32_t detect_cpu_frequency(void)
{
	uint64_t start, end, diff;
	uint64_t ticks, old;

	old = get_clock_tick();

	/* wait for the next time slice */
	while((ticks = get_clock_tick()) - old == 0)
		PAUSE;

	rmb();
	start = rdtsc();
	/* wait 3 ticks to determine the frequency */
	while(get_clock_tick() - ticks < 3)
		PAUSE;
	rmb();
	end = rdtsc();

	diff = end > start ? end - start : start - end;
	cpu_freq = (uint32_t) ((TIMER_FREQ*diff) / (1000000ULL*3ULL));

	return cpu_freq;
}

static int get_min_pstate(void)
{
	uint64_t value;

	value = rdmsr(MSR_PLATFORM_INFO);

	return (value >> 40) & 0xFF;
}

static int get_max_pstate(void)
{
	uint64_t value;

	value = rdmsr(MSR_PLATFORM_INFO);

	return (value >> 8) & 0xFF;
}

static uint8_t is_turbo = 0;
static int max_pstate, min_pstate;
static int turbo_pstate;

static int get_turbo_pstate(void)
{
	uint64_t value;
	int i, ret;

	value = rdmsr(MSR_NHM_TURBO_RATIO_LIMIT);
	i = get_max_pstate();
	ret = (value) & 255;
	if (ret < i)
		ret = i;

	return ret;
}

static void set_pstate(int pstate)
{
	uint64_t v = pstate << 8;
	if (is_turbo)
		v |= (1ULL << 32);
	wrmsr(MSR_IA32_PERF_CTL, v);
}

void dump_pstate(void)
{
	if (!has_est())
		return;

	LOG_INFO("P-State 0x%x - 0x%x, turbo 0x%x\n", min_pstate, max_pstate, turbo_pstate);
	LOG_INFO("PERF CTL 0x%llx\n", rdmsr(MSR_IA32_PERF_CTL));
	LOG_INFO("PERF STATUS 0x%llx\n", rdmsr(MSR_IA32_PERF_STATUS));
}

static void check_est(uint8_t out)
{
	uint32_t a=0, b=0, c=0, d=0;
	uint64_t v;

	if (!has_est())
		return;

	if (out)
		LOG_INFO("System supports Enhanced SpeedStep Technology\n");

	// enable Enhanced SpeedStep Technology
	v = rdmsr(MSR_IA32_MISC_ENABLE);
	if (!(v & MSR_IA32_MISC_ENABLE_ENHANCED_SPEEDSTEP)) {
		if (out)
			LOG_INFO("Linux doesn't enable Enhanced SpeedStep Technology\n");
		return;
	}

	if (v & MSR_IA32_MISC_ENABLE_SPEEDSTEP_LOCK) {
		if (out)
			LOG_INFO("Enhanced SpeedStep Technology is locked\n");
		return;
	}

	if (v & MSR_IA32_MISC_ENABLE_TURBO_DISABLE) {
		if (out)
			LOG_INFO("Turbo Mode is disabled\n");
	} else {
		if (out)
			LOG_INFO("Turbo Mode is enabled\n");
		is_turbo=1;
	}

	cpuid(6, &a, &b, &c, &d);
	if (c & CPU_FEATURE_IDA) {
		if (out)
			LOG_INFO("Found P-State hardware coordination feedback capability bit\n");
	}

	if (c & CPU_FEATURE_HWP) {
		if (out)
			LOG_INFO("P-State HWP enabled\n");
	}

	if (c & CPU_FEATURE_EPB) {
		// for maximum performance we have to clear BIAS
		wrmsr(MSR_IA32_ENERGY_PERF_BIAS, 0);
		if (out)
			LOG_INFO("Found Performance and Energy Bias Hint support: 0x%llx\n", rdmsr(MSR_IA32_ENERGY_PERF_BIAS));
	}

#if 0
	if (out) {
		LOG_INFO("CPU features 6: 0x%x, 0x%x, 0x%x, 0x%x\n", a, b, c, d);
		LOG_INFO("MSR_PLATFORM_INFO 0x%llx\n", rdmsr(MSR_PLATFORM_INFO));
	}
#endif

	max_pstate = get_max_pstate();
	min_pstate = get_min_pstate();
	turbo_pstate = get_turbo_pstate();

	// set maximum p-state to get peak performance
	if (is_turbo)
		set_pstate(turbo_pstate);
	else
		set_pstate(max_pstate);

	if (out)
		dump_pstate();

	return;
}
