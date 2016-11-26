/* Copyright (C) 2015 RWTH Aachen University, Germany.
   Contributed by Stefan Lankes <slankes@eonerc.rwth-aachen.de>.

   Libgomp is free software; you can redistribute it and/or modify it
   under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 3, or (at your option)
   any later version.

   Libgomp is distributed in the hope that it will be useful, but WITHOUT ANY
   WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
   FOR A PARTICULAR PURPOSE.  See the GNU General Public License for
   more details.

   Under Section 7 of GPL version 3, you are granted additional
   permissions described in the GCC Runtime Library Exception, version
   3.1, as published by the Free Software Foundation.

   You should have received a copy of the GNU General Public License and
   a copy of the GCC Runtime Library Exception along with this program;
   see the files COPYING3 and COPYING.RUNTIME respectively.  If not, see
   <http://www.gnu.org/licenses/>.  */

/*
 * This file contains system specific timer routines.  It is expected that
 * a system may well want to write special versions of each of these.
 */

#include "libgomp.h"
#include <unistd.h>

extern unsigned int get_cpufreq(void);
static unsigned long long start_tsc;

inline static unsigned long long rdtsc(void)
{
	unsigned long lo, hi;
	asm volatile ("rdtsc" : "=a"(lo), "=d"(hi) :: "memory");
	return ((unsigned long long) hi << 32ULL | (unsigned long long) lo);
}

__attribute__((constructor)) static void timer_init()
{
	start_tsc = rdtsc();
}

double
omp_get_wtime (void)
{
	double ret;

	ret = (double) (rdtsc() - start_tsc) / ((double) get_cpufreq() * 1000000.0);
	//printf("CPU frequency: %d MHz\n", get_cpufreq());

	return ret;
}

double
omp_get_wtick (void)
{
	return 1.0 / ((double) get_cpufreq() * 1000000.0);
}

ialias (omp_get_wtime)
ialias (omp_get_wtick)
