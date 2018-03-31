/*
* Copyright (c) 2017, Stefan Lankes, RWTH Aachen University
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

#define _GNU_SOURCE

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <errno.h>
#include <limits.h>

#include "proxy.h"

#ifdef __x86_64__
inline static void __cpuid(uint32_t code, uint32_t* a, uint32_t* b, uint32_t* c, uint32_t* d)
{
	__asm volatile ("cpuid" : "=a"(*a), "=b"(*b), "=c"(*c), "=d"(*d) : "0"(code), "2"(*c));
}

// Try to determine the frequency from the CPU brand.
// Code is derived from the manual "Intel Processor
// Identification and the CPUID Instruction".
static uint32_t get_frequency_from_brand(void)
{
	char cpu_brand[4*3*sizeof(uint32_t)+1] = {[0 ... 4*3*sizeof(uint32_t)] = 0};
	uint32_t* bint = (uint32_t*) cpu_brand;
	uint32_t index, multiplier = 0;
	uint32_t cpu_freq = 0;
	uint32_t extended;

	__cpuid(0x80000000, &extended, bint+1, bint+2, bint+3);
	if (extended < 0x80000004)
	return 0;

	__cpuid(0x80000002, bint+0, bint+1, bint+2, bint+3);
	__cpuid(0x80000003, bint+4, bint+5, bint+6, bint+7);
	__cpuid(0x80000004, bint+8, bint+9, bint+10, bint+11);

	for(index=0; index<sizeof(cpu_brand)-2; index++)
	{
		if ((cpu_brand[index+1] == 'H') && (cpu_brand[index+2] == 'z'))
		{
			if (cpu_brand[index] == 'M')
			multiplier = 1;
			else if (cpu_brand[index] == 'G')
			multiplier = 1000;
			else if (cpu_brand[index] == 'T')
			multiplier = 1000000;
		}

		if (multiplier > 0) {
			uint32_t freq;

			// Compute frequency (in MHz) from brand string
			if (cpu_brand[index-3] == '.') { // If format is “x.xx”
				freq  = (uint32_t)(cpu_brand[index-4] - '0') * multiplier;
				freq += (uint32_t)(cpu_brand[index-2] - '0') * (multiplier / 10);
				freq += (uint32_t)(cpu_brand[index-1] - '0') * (multiplier / 100);
			} else { // If format is xxxx
				freq  = (uint32_t)(cpu_brand[index-4] - '0') * 1000;
				freq += (uint32_t)(cpu_brand[index-3] - '0') * 100;
				freq += (uint32_t)(cpu_brand[index-2] - '0') * 10;
				freq += (uint32_t)(cpu_brand[index-1] - '0');
				freq *= multiplier;
			}

			return freq;
		}
	}

	return 0;
}
#endif

uint32_t get_cpufreq(void)
{
	char line[128];
	uint32_t freq = 0;
	char* match;

#ifdef __x86_64__
	freq = get_frequency_from_brand();
	if (freq > 0)
		return freq;
#endif

	// TODO: fallback solution, on some systems is cpuinfo_max_freq the turbo frequency
	// => wrong value
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

ssize_t pread_in_full(int fd, void *buf, size_t count, off_t offset)
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
