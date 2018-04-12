// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

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
