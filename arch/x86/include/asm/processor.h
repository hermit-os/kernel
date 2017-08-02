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

/**
 * @author Stefan Lankes
 * @file arch/x86/include/asm/processor.h
 * @brief CPU-specific functions
 *
 * This file contains structures and functions related to CPU-specific assembler commands.
 */

#ifndef __ARCH_PROCESSOR_H__
#define __ARCH_PROCESSOR_H__

#include <hermit/stddef.h>
#include <asm/gdt.h>
#include <asm/apic.h>
#include <asm/irqflags.h>
#include <asm/pci.h>
#include <asm/tss.h>

#ifdef __cplusplus
extern "C" {
#endif

// feature list 0x00000001 (ebx)
#define CPU_FEATURE_FPU			(1 << 0)
#define CPU_FEATURE_PSE			(1 << 3)
#define CPU_FEATURE_MSR			(1 << 5)
#define CPU_FEATURE_PAE			(1 << 6)
#define CPU_FEATURE_MCE			(1 << 7)
#define CPU_FEATURE_APIC		(1 << 9)
#define CPU_FEATURE_SEP			(1 << 11)
#define CPU_FEATURE_PGE			(1 << 13)
#define CPU_FEATURE_PAT			(1 << 16)
#define CPU_FEATURE_PSE36		(1 << 17)
#define CPU_FEATURE_CLFLUSH		(1 << 19)
#define CPU_FEATURE_MMX			(1 << 23)
#define CPU_FEATURE_FXSR		(1 << 24)
#define CPU_FEATURE_SSE			(1 << 25)
#define CPU_FEATURE_SSE2		(1 << 26)

// feature list 0x00000001 (ecx)
#define CPU_FEATURE_MWAIT			(1 << 3)
#define CPU_FEATURE_VMX				(1 << 5)
#define CPU_FEATURE_EST				(1 << 7)
#define CPU_FEATURE_SSE3			(1 << 9)
#define CPU_FEATURE_FMA				(1 << 12)
#define CPU_FEATURE_DCA				(1 << 18)
#define CPU_FEATURE_SSE4_1			(1 << 19)
#define CPU_FEATURE_SSE4_2			(1 << 20)
#define CPU_FEATURE_X2APIC			(1 << 21)
#define CPU_FEATURE_MOVBE			(1 << 22)
#define CPU_FEATURE_XSAVE			(1 << 26)
#define CPU_FEATURE_OSXSAVE			(1 << 27)
#define CPU_FEATURE_AVX				(1 << 28)
#define CPU_FEATURE_RDRAND			(1 << 30)
#define CPU_FEATURE_HYPERVISOR			(1 << 31)

// CPUID.80000001H:EDX feature list
#define CPU_FEATURE_SYSCALL			(1 << 11)
#define CPU_FEATURE_NX				(1 << 20)
#define CPU_FEATURE_1GBHP			(1 << 26)
#define CPU_FEATURE_RDTSCP			(1 << 27)
#define CPU_FEATURE_LM				(1 << 29)

// feature list 0x00000007:0
#define CPU_FEATURE_FSGSBASE			(1 << 0)
#define CPU_FEATURE_TSC_ADJUST			(1 << 1)
#define CPU_FEATURE_SGX			(1 << 2)
#define CPU_FEATURE_BMI1			(1 << 3)
#define CPU_FEATURE_HLE				(1 << 4)
#define CPU_FEATURE_AVX2			(1 << 5)
#define CPU_FEATURE_SMEP			(1 << 7)
#define CPU_FEATURE_BMI2			(1 << 8)
#define CPU_FEATURE_ERMS			(1 << 9)
#define CPU_FEATURE_INVPCID			(1 << 10)
#define CPU_FEATURE_RTM				(1 << 11)
#define CPU_FEATURE_CQM				(1 << 12)
#define CPU_FEATURE_MPX				(1 << 14)
#define CPU_FEATURE_AVX512F			(1 << 16)
#define CPU_FEATURE_RDSEED			(1 << 18)
#define CPU_FEATURE_ADX				(1 << 19)
#define CPU_FEATURE_SMAP			(1 << 20)
#define CPU_FEATURE_PCOMMIT			(1 << 22)
#define CPU_FEATURE_CLFLUSHOPT			(1 << 23)
#define CPU_FEATURE_CLWB			(1 << 24)
#define CPU_FEATURE_AVX512PF			(1 << 26)
#define CPU_FEATURE_AVX512ER			(1 << 27)
#define CPU_FEATURE_AVX512CD			(1 << 28)
#define CPU_FEATURE_SHA_NI			(1 << 29)
#define CPU_FEATURE_AVX512BW		(1 << 30)
#define CPU_FEATURE_AVX512VL		(1 <<31)

// feature list 0x00000006
#define CPU_FEATURE_IDA				(1 << 0)
#define CPU_FEATURE_EPB				(1 << 3)
#define CPU_FEATURE_HWP				(1 << 10)

/*
 * EFLAGS bits
 */
#define EFLAGS_CF	(1UL <<  0) /* Carry Flag */
#define EFLAGS_FIXED	(1UL <<  1) /* Bit 1 - always on */
#define EFLAGS_PF	(1UL <<  2) /* Parity Flag */
#define EFLAGS_AF	(1UL <<  4) /* Auxiliary carry Flag */
#define EFLAGS_ZF	(1UL <<  6) /* Zero Flag */
#define EFLAGS_SF	(1UL <<  7) /* Sign Flag */
#define EFLAGS_TF	(1UL <<  8) /* Trap Flag */
#define EFLAGS_IF	(1UL <<  9) /* Interrupt Flag */
#define EFLAGS_DF	(1UL << 10) /* Direction Flag */
#define EFLAGS_OF	(1UL << 11) /* Overflow Flag */
#define EFLAGS_IOPL	(1UL << 12) /* I/O Privilege Level (2 bits) */
#define EFLAGS_NT	(1UL << 14) /* Nested Task */
#define EFLAGS_RF	(1UL << 16) /* Resume Flag */
#define EFLAGS_VM	(1UL << 17) /* Virtual Mode */
#define EFLAGS_AC	(1UL << 18) /* Alignment Check/Access Control */
#define EFLAGS_VIF	(1UL << 19) /* Virtual Interrupt Flag */
#define EFLAGS_VIP	(1UL << 20) /* Virtual Interrupt Pending */
#define EFLAGS_ID	(1UL << 21) /* CPUID detection */


// x86 control registers

/// Protected Mode Enable
#define CR0_PE					(1 << 0)
/// Monitor coprocessor
#define CR0_MP					(1 << 1)
/// Enable FPU emulation
#define CR0_EM					(1 << 2)
/// Task switched
#define CR0_TS					(1 << 3)
/// Extension type of coprocessor
#define CR0_ET					(1 << 4)
/// Enable FPU error reporting
#define CR0_NE					(1 << 5)
/// Enable write protected pages
#define CR0_WP					(1 << 16)
/// Enable alignment checks
#define CR0_AM					(1 << 18)
/// Globally enables/disable write-back caching
#define CR0_NW					(1 << 29)
/// Globally disable memory caching
#define CR0_CD					(1 << 30)
/// Enable paging
#define CR0_PG					(1 << 31)

/// Virtual 8086 Mode Extensions
#define CR4_VME					(1 << 0)
/// Protected-mode Virtual Interrupts
#define CR4_PVI					(1 << 1)
/// Disable Time Stamp Counter register (rdtsc instruction)
#define CR4_TSD					(1 << 2)
/// Enable debug extensions
#define CR4_DE					(1 << 3)
///  Enable hugepage support
#define CR4_PSE					(1 << 4)
/// Enable physical address extension
#define CR4_PAE					(1 << 5)
/// Enable machine check exceptions
#define CR4_MCE					(1 << 6)
/// Enable global pages
#define CR4_PGE					(1 << 7)
/// Enable Performance-Monitoring Counter
#define CR4_PCE					(1 << 8)
/// Enable Operating system support for FXSAVE and FXRSTOR instructions
#define CR4_OSFXSR				(1 << 9)
/// Enable Operating System Support for Unmasked SIMD Floating-Point Exceptions
#define CR4_OSXMMEXCPT			(1 << 10)
/// Enable Virtual Machine Extensions, see Intel VT-x
#define CR4_VMXE				(1 << 13)
/// Enable Safer Mode Extensions, see Trusted Execution Technology (TXT)
#define CR4_SMXE				(1 << 14)
/// Enables the instructions RDFSBASE, RDGSBASE, WRFSBASE, and WRGSBASE
#define CR4_FSGSBASE				(1 << 16)
/// Enables process-context identifiers
#define CR4_PCIDE				(1 << 17)
/// Enable XSAVE and Processor Extended States
#define CR4_OSXSAVE				(1 << 18)
/// Enable Supervisor Mode Execution Protection
#define CR4_SMEP				(1 << 20)
/// Enable Supervisor Mode Access Protection
#define CR4_SMAP				(1 << 21)

// x86-64 specific MSRs

/// APIC register
#define MSR_APIC_BASE				0x0000001B
/// extended feature register
#define MSR_EFER				0xc0000080
/// legacy mode SYSCALL target
#define MSR_STAR				0xc0000081
/// long mode SYSCALL target
#define MSR_LSTAR				0xc0000082
/// compat mode SYSCALL target
#define MSR_CSTAR				0xc0000083
/// EFLAGS mask for syscall
#define MSR_SYSCALL_MASK			0xc0000084
/// 64bit FS base
#define MSR_FS_BASE				0xc0000100
/// 64bit GS base
#define MSR_GS_BASE				0xc0000101
/// SwapGS GS shadow
#define MSR_KERNEL_GS_BASE			0xc0000102

#define MSR_XAPIC_ENABLE			(1UL << 11)
#define MSR_X2APIC_ENABLE			(1UL << 10)

#define MSR_IA32_PLATFORM_ID			0x00000017

#define MSR_IA32_PERFCTR0			0x000000c1
#define MSR_IA32_PERFCTR1			0x000000c2
#define MSR_FSB_FREQ				0x000000cd
#define MSR_PLATFORM_INFO			0x000000ce

#define MSR_IA32_MPERF				0x000000e7
#define MSR_IA32_APERF				0x000000e8
#define MSR_IA32_MISC_ENABLE			0x000001a0
#define MSR_IA32_FEATURE_CONTROL		0x0000003a
#define MSR_IA32_ENERGY_PERF_BIAS		0x000001b0
#define MSR_IA32_PERF_STATUS			0x00000198
#define MSR_IA32_PERF_CTL			0x00000199
#define MSR_IA32_CR_PAT				0x00000277
#define MSR_MTRRdefType				0x000002ff

#define MSR_PPERF				0x0000064e
#define MSR_PERF_LIMIT_REASONS			0x0000064f
#define MSR_PM_ENABLE				0x00000770
#define MSR_HWP_CAPABILITIES			0x00000771
#define MSR_HWP_REQUEST_PKG			0x00000772
#define MSR_HWP_INTERRUPT			0x00000773
#define MSR_HWP_REQUEST				0x00000774
#define MSR_HWP_STATUS				0x00000777

#define MSR_IA32_MISC_ENABLE_ENHANCED_SPEEDSTEP	(1ULL << 16)
#define MSR_IA32_MISC_ENABLE_SPEEDSTEP_LOCK	(1ULL << 20)
#define MSR_IA32_MISC_ENABLE_TURBO_DISABLE	(1ULL << 38)

#define MSR_MTRRfix64K_00000			0x00000250
#define MSR_MTRRfix16K_80000			0x00000258
#define MSR_MTRRfix16K_A0000			0x00000259
#define MSR_MTRRfix4K_C0000			0x00000268
#define MSR_MTRRfix4K_C8000			0x00000269
#define MSR_MTRRfix4K_D0000			0x0000026a
#define MSR_MTRRfix4K_D8000			0x0000026b
#define MSR_MTRRfix4K_E0000			0x0000026c
#define MSR_MTRRfix4K_E8000			0x0000026d
#define MSR_MTRRfix4K_F0000			0x0000026e
#define MSR_MTRRfix4K_F8000			0x0000026f

#define MSR_OFFCORE_RSP_0			0x000001a6
#define MSR_OFFCORE_RSP_1			0x000001a7
#define MSR_NHM_TURBO_RATIO_LIMIT		0x000001ad
#define MSR_IVT_TURBO_RATIO_LIMIT		0x000001ae
#define MSR_TURBO_RATIO_LIMIT			0x000001ad
#define MSR_TURBO_RATIO_LIMIT1			0x000001ae
#define MSR_TURBO_RATIO_LIMIT2			0x000001af

// MSR EFER bits
#define EFER_SCE				(1 << 0)
#define EFER_LME				(1 << 8)
#define EFER_LMA				(1 << 10)
#define EFER_NXE				(1 << 11)
#define EFER_SVME				(1 << 12)
#define EFER_LMSLE				(1 << 13)
#define EFER_FFXSR				(1 << 14)
#define EFER_TCE				(1 << 15)

typedef struct {
	uint32_t feature1, feature2;
	uint32_t feature3, feature4;
	uint32_t addr_width;
} cpu_info_t;

extern cpu_info_t cpu_info;

// determine the cpu features
int cpu_detection(void);

inline static uint32_t has_fpu(void) {
	return (cpu_info.feature1 & CPU_FEATURE_FPU);
}

inline static uint32_t has_msr(void) {
	return (cpu_info.feature1 & CPU_FEATURE_MSR);
}

inline static uint32_t has_mce(void) {
	return (cpu_info.feature1 & CPU_FEATURE_MCE);
}

inline static uint32_t has_apic(void) {
	return (cpu_info.feature1 & CPU_FEATURE_APIC);
}

inline static uint32_t has_fxsr(void) {
	return (cpu_info.feature1 & CPU_FEATURE_FXSR);
}

inline static uint32_t has_clflush(void) {
	return (cpu_info.feature1 & CPU_FEATURE_CLFLUSH);
}

inline static uint32_t has_sse(void) {
	return (cpu_info.feature1 & CPU_FEATURE_SSE);
}

inline static uint32_t has_pat(void) {
	return (cpu_info.feature1 & CPU_FEATURE_PAT);
}

inline static uint32_t has_sse2(void) {
	return (cpu_info.feature1 & CPU_FEATURE_SSE2);
}

inline static uint32_t has_pge(void)
{
	return (cpu_info.feature1 & CPU_FEATURE_PGE);
}

inline static uint32_t has_sep(void) {
	return (cpu_info.feature1 & CPU_FEATURE_SEP);
}

inline static uint32_t has_movbe(void) {
	return (cpu_info.feature2 & CPU_FEATURE_MOVBE);
}

inline static uint32_t has_fma(void) {
	return (cpu_info.feature2 & CPU_FEATURE_FMA);
}

inline static uint32_t has_mwait(void) {
	return (cpu_info.feature2 & CPU_FEATURE_MWAIT);
}

inline static uint32_t has_vmx(void) {
	return (cpu_info.feature2 & CPU_FEATURE_VMX);
}

inline static uint32_t has_est(void)
{
	return (cpu_info.feature2 & CPU_FEATURE_EST);
}

inline static uint32_t has_sse3(void) {
	return (cpu_info.feature2 & CPU_FEATURE_SSE3);
}

inline static uint32_t has_dca(void) {
	return (cpu_info.feature2 & CPU_FEATURE_DCA);
}

inline static uint32_t has_sse4_1(void) {
	return (cpu_info.feature2 & CPU_FEATURE_SSE4_1);
}

inline static uint32_t has_sse4_2(void) {
	return (cpu_info.feature2 & CPU_FEATURE_SSE4_2);
}

inline static uint32_t has_x2apic(void) {
	return (cpu_info.feature2 & CPU_FEATURE_X2APIC);
}

inline static uint32_t has_xsave(void) {
	return (cpu_info.feature2 & CPU_FEATURE_XSAVE);
}

inline static uint32_t has_osxsave(void) {
	return (cpu_info.feature2 & CPU_FEATURE_OSXSAVE);
}

inline static uint32_t has_avx(void) {
	return (cpu_info.feature2 & CPU_FEATURE_AVX);
}

inline static uint32_t has_rdrand(void) {
	return (cpu_info.feature2 & CPU_FEATURE_RDRAND);
}

inline static uint32_t on_hypervisor(void) {
	return (cpu_info.feature2 & CPU_FEATURE_HYPERVISOR);
}

inline static uint32_t has_nx(void)
{
	return (cpu_info.feature3 & CPU_FEATURE_NX);
}

inline static uint32_t has_fsgsbase(void) {
	return (cpu_info.feature4 & CPU_FEATURE_FSGSBASE);
}

inline static uint32_t has_sgx(void) {
	return (cpu_info.feature4 & CPU_FEATURE_SGX);
}

inline static uint32_t has_avx2(void) {
	return (cpu_info.feature4 & CPU_FEATURE_AVX2);
}

inline static uint32_t has_bmi1(void) {
	return (cpu_info.feature4 & CPU_FEATURE_BMI1);
}

inline static uint32_t has_bmi2(void) {
	return (cpu_info.feature4 & CPU_FEATURE_BMI2);
}

inline static uint32_t has_hle(void) {
	return (cpu_info.feature4 & CPU_FEATURE_HLE);
}

inline static uint32_t has_cqm(void) {
	return (cpu_info.feature4 & CPU_FEATURE_CQM);
}

inline static uint32_t has_rtm(void) {
	return (cpu_info.feature4 & CPU_FEATURE_RTM);
}

inline static uint32_t has_clflushopt(void) {
	return (cpu_info.feature4 & CPU_FEATURE_CLFLUSHOPT);
}

inline static uint32_t has_clwb(void) {
	return (cpu_info.feature4 & CPU_FEATURE_CLWB);
}

inline static uint32_t has_avx512f(void) {
	return (cpu_info.feature4 & CPU_FEATURE_AVX512F);
}

inline static uint32_t has_avx512pf(void) {
	return (cpu_info.feature4 & CPU_FEATURE_AVX512PF);
}

inline static uint32_t has_avx512er(void) {
	return (cpu_info.feature4 & CPU_FEATURE_AVX512ER);
}

inline static uint32_t has_avx512cd(void) {
	return (cpu_info.feature4 & CPU_FEATURE_AVX512CD);
}

inline static uint32_t has_avx512bw(void) {
	return (cpu_info.feature4 & CPU_FEATURE_AVX512BW);
}

inline static uint32_t has_avx512vl(void) {
	return (cpu_info.feature4 & CPU_FEATURE_AVX512VL);
}

inline static uint32_t has_rdtscp(void) {
	return (cpu_info.feature3 & CPU_FEATURE_RDTSCP);
}

/// clear TS bit in cr0
static inline void clts(void)
{
	asm volatile("clts");
}

/** @brief Read a random number
 *
 *  Returns a hardware generated random value.
 */
inline static uint32_t rdrand(void)
{
	uint32_t val;
	uint8_t rc;

	do {
		asm volatile("rdrand %0 ; setc %1" : "=r" (val), "=qm" (rc));
	} while(rc == 0); // rc == 0: underflow

	return val;
}

/** @brief Read out time stamp counter
 *
 * The rdtsc instruction puts a 64 bit time stamp value
 * into EDX:EAX.
 *
 * @return The 64 bit time stamp value
 */
inline static uint64_t rdtsc(void)
{
	uint32_t lo, hi;

	asm volatile ("rdtsc" : "=a"(lo), "=d"(hi) :: "memory");

	return ((uint64_t)hi << 32ULL | (uint64_t)lo);
}

/** @brief Read time stamp counter and processor id
 *
 * The rdtscp instruction puts a 64 bit trime stamp value
 * into EDX:EAX and the processor id into ECX.
 *
 * @return The 64 bit time stamp value
 */
inline static uint64_t rdtscp(uint32_t* cpu_id)
{
	uint32_t lo, hi;
	uint32_t id;

	asm volatile ("rdtscp" : "=a"(lo), "=c"(id), "=d"(hi));
	if (cpu_id)
		*cpu_id = id;

	return ((uint64_t)hi << 32ULL | (uint64_t)lo);
}

inline static uint64_t get_rdtsc()
{
	return has_rdtscp() ? rdtscp(NULL) : rdtsc();
}

/** @brief Read MSR
 *
 * The asm instruction rdmsr which stands for "Read from model specific register"
 * is used here.
 *
 * @param msr The parameter which rdmsr assumes in ECX
 * @return The value rdmsr put into EDX:EAX
 */
inline static uint64_t rdmsr(uint32_t msr) {
	uint32_t low, high;

	asm volatile ("rdmsr" : "=a" (low), "=d" (high) : "c" (msr));

	return ((uint64_t)high << 32) | low;
}

/** @brief Write a value to a  Machine-Specific Registers (MSR)
 *
 * The asm instruction wrmsr which stands for "Write to model specific register"
 * is used here.
 *
 * @param msr The MSR identifier
 * @param value Value, which will be store in the MSR
 */
inline static void wrmsr(uint32_t msr, uint64_t value)
{
	uint32_t low =  (uint32_t) value;
	uint32_t high = (uint32_t) (value >> 32);

	asm volatile("wrmsr" :: "c"(msr), "a"(low), "d"(high) : "memory");
}

/** @brief Read cr0 register
 * @return cr0's value
 */
static inline size_t read_cr0(void) {
	size_t val;
	asm volatile("mov %%cr0, %0" : "=r"(val) :: "memory");
	return val;
}

/** @brief Write a value into cr0 register
 * @param val The value you want to write into cr0
 */
static inline void write_cr0(size_t val) {
	asm volatile("mov %0, %%cr0" :: "r"(val) : "memory");
}

/** @brief Read cr2 register
 * @return cr2's value
 */
static inline size_t read_cr2(void) {
	size_t val;
	asm volatile("mov %%cr2, %0" : "=r"(val) :: "memory");
	return val;
}

/** @brief Write a value into cr2 register
 * @param val The value you want to write into cr2
 */
static inline void write_cr2(size_t val) {
	asm volatile("mov %0, %%cr2" :: "r"(val) : "memory");
}

/** @brief Read cr3 register
 * @return cr3's value
 */
static inline size_t read_cr3(void) {
        size_t val;
        asm volatile("mov %%cr3, %0" : "=r"(val) :: "memory");
        return val;
}

/** @brief Write a value into cr3 register
 * @param val The value you want to write into cr3
 */
static inline void write_cr3(size_t val) {
	asm volatile("mov %0, %%cr3" :: "r"(val) : "memory");
}

/** @brief Read cr4 register
 * @return cr4's value
 */
static inline size_t read_cr4(void) {
	size_t val;
	asm volatile("mov %%cr4, %0" : "=r"(val) :: "memory");
	return val;
}

/** @brief Write a value into cr4 register
 * @param val The value you want to write into cr4
 */
static inline void write_cr4(size_t val) {
	asm volatile("mov %0, %%cr4" :: "r"(val) : "memory");
}

static inline size_t read_cr8(void)
{
	size_t val;
	asm volatile("mov %%cr8, %0" : "=r" (val) :: "memory");
	return val;
}

static inline void write_cr8(size_t val)
{
	asm volatile("movq %0, %%cr8" :: "r" (val) : "memory");
}

typedef size_t (*func_read_fsgs)(void);
typedef void (*func_write_fsgs)(size_t);

extern func_read_fsgs readfs;
extern func_read_fsgs readgs;
extern func_write_fsgs writefs;
extern func_write_fsgs writegs;

/** @brife Get thread local storage
 *
 * Helper function to get the TLS of the current task
 */
static inline size_t get_tls(void)
{
	return readfs();
}

/** @brief Set thread local storage
 *
 * Helper function to set the TLS of the current task
 */
static inline void set_tls(size_t addr)
{
	writefs(addr);
}

/** @brief Flush cache
 *
 * The wbinvd asm instruction which stands for "Write back and invalidate"
 * is used here
 */
inline static void flush_cache(void) {
	asm volatile ("wbinvd" ::: "memory");
}

/** @brief Invalidate cache
 *
 * The invd asm instruction which invalidates cache without writing back
 * is used here
 */
inline static void invalidate_cache(void) {
	asm volatile ("invd" ::: "memory");
}

/// Send IPIs to the other core, which flush the TLB on the other cores.
int ipi_tlb_flush(void);

/** @brief Flush Translation Lookaside Buffer
 *
 * Just reads cr3 and writes the same value back into it.
 */
static inline void tlb_flush(uint8_t with_ipi)
{
	size_t val = read_cr3();

	if (val)
		write_cr3(val);

#if MAX_CORES > 1
	if (with_ipi)
		ipi_tlb_flush();
#endif
}

/** @brief Flush a specific page entry in TLB
 * @param addr The (virtual) address of the page to flush
 */
static inline void tlb_flush_one_page(size_t addr, uint8_t with_ipi)
{
	asm volatile("invlpg (%0)" : : "r"(addr) : "memory");

#if MAX_CORES > 1
	if (with_ipi)
		ipi_tlb_flush();
#endif
}

/** @brief Invalidate cache
 *
 * The invd asm instruction which invalidates cache without writing back
 * is used here
 */
inline static void invalid_cache(void) {
	asm volatile ("invd");
}

static inline void monitor(const void *eax, unsigned long ecx, unsigned long edx)
{
	asm volatile("monitor" :: "a" (eax), "c" (ecx), "d"(edx));
}

static inline void mwait(unsigned long eax, unsigned long ecx)
{
	asm volatile("mwait" :: "a" (eax), "c" (ecx));
}

static inline void clflush(volatile void *addr)
{
	asm volatile("clflush %0" : "+m" (*(volatile char *)addr));
}

static inline void clwb(volatile void *addr)
{
	asm volatile("clwb %0" : "+m" (*(volatile char *)addr));
}

static inline void  clflushopt(volatile void *addr)
{
	asm volatile("clflushopt %0" : "+m" (*(volatile char *)addr));
}

#if 0
// the old way to serialize the store and load operations
static inline void mb(void) { asm volatile ("lock; addl $0,0(%%esp)" ::: "memory", "cc"); }
static inline void rmb(void) { asm volatile ("lock; addl $0,0(%%esp)" ::: "memory", "cc"); }
static inline void wmb(void) { asm volatile ("lock; addl $0,0(%%esp)" ::: "memory", "cc"); }
#else
/// Force strict CPU ordering, serializes load and store operations.
static inline void mb(void) { asm volatile("mfence":::"memory"); }
/// Force strict CPU ordering, serializes load operations.
static inline void rmb(void) { asm volatile("lfence":::"memory"); }
/// Force strict CPU ordering, serializes store operations.
static inline void wmb(void) { asm volatile("sfence" ::: "memory"); }
#endif

/** @brief Get Extended Control Register
 *
 * Reads the contents of the extended control register (XCR) specified
 * in the ECX register.
 */
static inline uint64_t xgetbv(uint32_t index)
{
	uint32_t edx, eax;

	asm volatile ("xgetbv" : "=a"(eax), "=d"(edx) : "c"(index));

	return (uint64_t) eax | ((uint64_t) edx << 32ULL);
}

/** @brief Set Extended Control Register
 *
 * Writes a 64-bit value into the extended control register (XCR) specified
 * in the ECX register.
 */
static inline void xsetbv(uint32_t index, uint64_t value)
{
	uint32_t edx, eax;

	edx = (uint32_t) (value >> 32ULL);
	eax = (uint32_t) value;

	asm volatile ("xsetbv" :: "a"(eax), "c"(index), "d"(edx));
}

/** @brief Read out CPU ID
 *
 * The cpuid asm-instruction does fill some information into registers and
 * this function fills those register values into the given uint32_t vars.\n
 *
 * @param code Input parameter for the cpuid instruction. Take a look into the intel manual.
 * @param a EAX value will be stores here
 * @param b EBX value will be stores here
 * @param c ECX value will be stores here
 * @param d EDX value will be stores here
 */
inline static void cpuid(uint32_t code, uint32_t* a, uint32_t* b, uint32_t* c, uint32_t* d) {
	asm volatile ("cpuid" : "=a"(*a), "=b"(*b), "=c"(*c), "=d"(*d) : "0"(code), "2"(*c));
}

/** @brief Read EFLAGS
 *
 * @return The EFLAGS value
 */
static inline uint64_t read_rflags(void)
{
	uint64_t result;
	asm volatile ("pushfq; pop %0" : "=r"(result));
	return result;
}

/* For KVM hypercalls, a three-byte sequence of either the vmcall or the vmmcall
 * instruction.  The hypervisor may replace it with something else but only the
 * instructions are guaranteed to be supported.
 *
 * Up to four arguments may be passed in rbx, rcx, rdx, and rsi respectively.
 * The hypercall number should be placed in rax and the return value will be
 * placed in rax.  No other registers will be clobbered unless explicitly
 * noted by the particular hypercall.
 */

inline static size_t vmcall0(int nr)
{
        size_t res;

	asm volatile ("vmcall" : "=a" (res): "a" (nr)
	                       : "memory");

	return res;
}

inline static size_t vmcall1(int nr, size_t arg0)
{
        size_t res;

	asm volatile ("vmcall" : "=a" (res): "a" (nr), "b"(arg0)
	                       : "memory");

	return res;
}

inline static size_t vmcall2(int nr, size_t arg0, size_t arg1)
{
        size_t res;

	asm volatile ("vmcall" : "=a" (res): "a" (nr), "b"(arg0), "c"(arg1)
	                       : "memory");

	return res;
}

inline static size_t vmcall3(int nr, size_t arg0, size_t arg1, size_t arg2)
{
        size_t res;

	asm volatile ("vmcall" : "=a" (res): "a" (nr), "b"(arg0), "c"(arg1), "d"(arg2)
	                       : "memory");

	return res;
}

inline static size_t vmcall4(int nr, size_t arg0, size_t arg1, size_t arg2, size_t arg3)
{
        size_t res;

	asm volatile ("vmcall" : "=a" (res): "a" (nr), "b"(arg0), "c"(arg1), "d"(arg2), "S"(arg3)
	                       : "memory");

	return res;
}

/** @brief search the first most significant bit
 *
 * @param i source operand
 * @return
 * - first bit, which is set in the source operand
 * - invalid value, if not bit ist set
 */
static inline size_t msb(size_t i)
{
	size_t ret;

	if (!i)
		return (sizeof(size_t)*8);
	asm volatile ("bsr %1, %0" : "=r"(ret) : "r"(i) : "cc");

	return ret;
}

/** @brief search the least significant bit
 *
 * @param i source operand
 * @return
 * - first bit, which is set in the source operand
 * - invalid value, if not bit ist set
 */
static inline size_t lsb(size_t i)
{
	size_t ret;

	if (!i)
		return (sizeof(size_t)*8);
	asm volatile ("bsf %1, %0" : "=r"(ret) : "r"(i) : "cc");

	return ret;
}

/** @brief: print current pstate
 */
void dump_pstate(void);

/// A one-instruction-do-nothing
#define NOP	asm  volatile ("nop")
/// The PAUSE instruction provides a hint to the processor that the code sequence is a spin-wait loop.
#define PAUSE	asm volatile ("pause")
/// The HALT instruction stops the processor until the next interrupt arrives
#define HALT	asm volatile ("hlt")

/** @brief Init several subsystems
 *
 * This function calls the initialization procedures for:
 * - GDT
 * - APIC
 * - PCI [if configured]
 *
 * @return 0 in any case
 */
inline static int system_init(void)
{
	gdt_install();
	cpu_detection();

	return 0;
}

/** @brief Detect and read out CPU frequency
 *
 * @return The CPU frequency in MHz
 */
uint32_t detect_cpu_frequency(void);

/** @brief Read out CPU frequency if detected before
 *
 * If you did not issue the detect_cpu_frequency() function before,
 * this function will call it implicitly.
 *
 * @return The CPU frequency in MHz
 */
uint32_t get_cpu_frequency(void);

/** @brief Busywait an microseconds interval of time
 * @param usecs The time to wait in microseconds
 */
void udelay(uint32_t usecs);

/// Register a task's TSS at GDT
static inline void register_task(void)
{
	uint16_t sel = (apic_cpu_id()*2+7) << 3;

	asm volatile ("ltr %%ax" : : "a"(sel));
}

/** @brief System calibration
 *
 * This procedure will detect the CPU frequency and calibrate the APIC timer.
 *
 * @return 0 in any case.
 */
inline static int system_calibration(void)
{
	size_t cr0;

	apic_init();
	if (is_single_kernel() && !is_uhyve())
		pci_init();
	register_task();

	// set task switched flag for the first FPU access
	//  => initialize the FPU
	cr0 = read_cr0();
	cr0 |= CR0_TS;
	write_cr0(cr0);

	irq_enable();
	detect_cpu_frequency();
	apic_calibration();

	return 0;
}

#ifdef __cplusplus
}
#endif

#endif
