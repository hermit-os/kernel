// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

///////////////////////////////////////
//// HERMITCORE RUST-SPECIFIC CODE ////

#include <hermit/syscall.h>

inline static void create_second_task(void (*entry_point)(void*))
{
	sys_spawn(NULL, entry_point, NULL, HIGH_PRIO, 0);
}

inline static void consume_task_time(void)
{
	// Spending >10ms in the second task guarantees that the scheduler
	// switches back to the first task on sys_yield().
	// Calling sys_msleep(ms) with ms < 10 enforces busy-waiting!
	sys_msleep(6);
	sys_msleep(6);
}

inline static void switch_task(void)
{
	sys_yield();
}


///////////////////////////////////
//// THE ACTUAL BENCHMARK CODE ////

#include <stdbool.h>
#include <stdio.h>

// You can enable this for debugging without any effect on the measurement.
//#define DEBUG_MESSAGES

#define N		1000
static bool finished;
static unsigned long long start;
static unsigned long long sum;

inline static unsigned long long rdtsc(void)
{
	unsigned long lo, hi;
	asm volatile ("rdtsc" : "=a"(lo), "=d"(hi) :: "memory");
	return ((unsigned long long) hi << 32ULL | (unsigned long long) lo);
}

void second_task(void* arg)
{
	unsigned long long end;

	for(;;)
	{
		// Calculate the cycle difference and add it to the sum.
		end = rdtsc();
		sum += (end - start);

		// Check if the benchmark has finished and we can end the second task.
		if (finished)
		{
			break;
		}

#ifdef DEBUG_MESSAGES
		printf("Hello from task 2\n");
#endif

		consume_task_time();

		// Save the current Time Stamp Counter value and switch back to the
		// first task.
		start = rdtsc();
		switch_task();
	}
}

int main(int argc, char** argv)
{
	int i;
	unsigned long long end;

	// Start the second task with the same priority on the boot processor.
	create_second_task(second_task);

	// Initialize the benchmark.
	printf("taskswitch test\n");
	printf("===============\n");

	finished = false;
	sum = 0;

	// Warm up
	switch_task();
	switch_task();

	// Run the benchmark.
	sum = 0;
	for(i = 0; i < N; i++)
	{
#ifdef DEBUG_MESSAGES
		printf("Hello from task 1\n");
#endif

		consume_task_time();

		// Save the current Time Stamp Counter value and switch to the second
		// task.
		start = rdtsc();
		switch_task();

		// Calculate the cycle difference and add it to the sum.
		end = rdtsc();
		sum += (end - start);
	}

	// Calculate and print the results.
	// In every loop iteration, task 1 switches to task 2 and task 2 switches
	// back to task 1.
	// Therefore, the total number needs to be divided by 2.
	printf("Average time for a task switch: %lld cycles\n", sum / (N * 2));

	// Finish the second task gracefully.
	finished = true;
	switch_task();

	return 0;
}
