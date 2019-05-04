/*
 * Copyright (c) 2016, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
 * http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
 * http://opensource.org/licenses/MIT>, at your option. This file may not be
 * copied, modified, or distributed except according to those terms.
 */

#ifndef __hermit__
#define _GNU_SOURCE
#endif

#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <sched.h>
#ifndef __hermit__
#include <sys/syscall.h>

static inline long mygetpid(void)
{
	return syscall(__NR_getpid);
}
#else
static inline long mygetpid(void)
{
	return getpid();
}

int sched_yield(void);
#endif

#define N		10000
#define M		(256+1)
#define BUFFSZ		(1ULL*1024ULL*1024ULL)

static char* buff[M];

#if 1
inline static unsigned long long rdtsc(void)
{
	unsigned long lo, hi;
	asm volatile ("rdtsc" : "=a"(lo), "=d"(hi) :: "memory");
	return ((unsigned long long) hi << 32ULL | (unsigned long long) lo);
}
#else
inline static unsigned long long rdtsc(void)
{
	unsigned int lo, hi;
	unsigned int id;

	asm volatile ("rdtscp" : "=a"(lo), "=c"(id), "=d"(hi));

	return ((unsigned long long)hi << 32ULL | (unsigned long long)lo);
}
#endif

int main(int argc, char** argv)
{
	long i, j, ret;
	unsigned long long start, end;
	const char str[] = "H";
	size_t len = strlen(str);

	printf("Determine systems performance\n");
	printf("=============================\n");

	// cache warm-up
	ret = mygetpid();
	ret = mygetpid();

	start = rdtsc();
	for(i=0; i<N; i++)
		ret = mygetpid();
	end = rdtsc();

	printf("Average time for getpid: %lld cycles, pid %ld\n", (end - start) / N, ret);

	// cache warm-up
	sched_yield();
	sched_yield();

	start = rdtsc();
	for(i=0; i<N; i++)
		sched_yield();
	end = rdtsc();

	printf("Average time for sched_yield: %lld cycles\n", (end - start) / N);

	// cache warm-up
	buff[0] = (char*) malloc(BUFFSZ);

	start = rdtsc();
	for(i=1; i<M; i++)
		buff[i] = (char*) malloc(BUFFSZ);
	end = rdtsc();

	printf("Average time for malloc: %lld cycles\n", (end - start) / (M-1));

	// cache warm-up
	for(j=0; j<BUFFSZ; j+=4096)
		buff[0][j] = '1';

	start = rdtsc();
	for(i=1; i<M; i++)
		for(j=0; j<BUFFSZ; j+=4096)
			buff[i][j] = '1';
	end = rdtsc();

	printf("Average time for the first page access: %lld cycles\n", (end - start) / ((M-1)*BUFFSZ/4096));

#if 0
	write(2, (const void *)str, len);
	write(2, (const void *)str, len);
	start = rdtsc();
	for(i=0; i<N; i++)
		write(2, (const void *)str, len);
	end = rdtsc();

	printf("\nAverage time for write: %lld cycles\n", (end - start) / N);
#endif

	return 0;
}
