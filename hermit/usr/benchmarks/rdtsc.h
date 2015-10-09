/*
 * =====================================================================================
 *
 *       Filename:  rdtsc.c
 *
 *    Description:  
 *
 *        Version:  1.0
 *        Created:  24.01.2011 13:41:30
 *       Revision:  none
 *       Compiler:  gcc
 *
 *         Author:  Georg Wassen (gw) (), 
 *        Company:  
 *
 * =====================================================================================
 */

#ifndef RDTSC_H
#define RDTSC_H

#include <stdint.h>

/*
 * rdtsc_ticks_per_sec(): measure frequency. Takes below a second.
 */
uint64_t rdtsc_ticks_per_sec(void) __attribute__((optimize(3)));


typedef union __attribute__((__transparent_union__))
{
    uint64_t *__u64;
    struct {uint32_t __low; uint32_t __high;} *__u32;
} tsc_t;

/* 
 * rdtsc(): Register-save version (compiler may insert additional push/pop)
 *          (clobbered-registers not given b/c compiler deduces from output-registers)
 */
static inline void __attribute__((__always_inline__, gnu_inline, optimize(3))) rdtsc(tsc_t tsc)
{
    __asm__ volatile ("rdtsc" : "=a"(tsc.__u32->__low), "=d"(tsc.__u32->__high) );
}

/* 
 * rdtsc_serial(): get RDTSC with leading and rear serializing LFENCE instruction
 */
static inline void __attribute__((__always_inline__, gnu_inline, optimize(3))) rdtsc_serialized(tsc_t tsc)
{
#if ! __MIC__
    __asm__ volatile (
            "lfence\n\t"    // serialize (needs SSE2, available since AMD Athlon64, Intel Core)
            "rdtsc\n\t"
            "lfence\n\t"
            : "=a"(tsc.__u32->__low), "=d"(tsc.__u32->__high) );
#else
    __asm__ volatile (
            "lock; add $0, 0(%%rsp)\n\t"    // serialize 
            "rdtsc\n\t"
            "lock; add $0, 0(%%rsp)\n\t"    // serialize 
            : "=a"(tsc.__u32->__low), "=d"(tsc.__u32->__high) :: "memory" );
#endif
}

void rdtsc_loop(uint64_t ticks);
void rdtsc_loop_sec(unsigned seconds);
uint64_t rdtsc_max_freq(int id);

int rdtsc_is_invariant(void);
uint64_t rdtsc_get_overhead(uint64_t iterations);
uint64_t rdtsc_get_overhead_serialized(uint64_t iterations);

#endif
