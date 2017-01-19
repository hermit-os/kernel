/*
 * Copyright (c) 2016, Daniel Krebs, RWTH Aachen University
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

#include <stdio.h>
#include <errno.h>
#include <signal.h>
#include <pthread.h>
#include <hermit/syscall.h>

#define THREAD_COUNT_DEFAULT 2

static volatile __thread int alive = 1;
static volatile __thread int thread_id;

pthread_barrier_t barrier;
pthread_barrierattr_t attr;

static void sighandler(int sig)
{
	printf("[%d] Received signal %d\n", thread_id, sig);
	alive = 0;
}

void* thread_func(void* arg)
{
	thread_id = *((int*) arg);

	printf("[%d] Hello (task ID: %d)\n", thread_id, sys_getpid());

	// register signal handler
	signal(16, sighandler);

	// make sure all threads are running before main threads starts sending
	// signals
	pthread_barrier_wait(&barrier);

	// stay here until signal received
	while(alive);

	printf("[%d] I'm done\n", thread_id);

	return 0;
}

int main(int argc, char** argv)
{
	size_t thread_count = THREAD_COUNT_DEFAULT;
	if(argc == 2) {
		thread_count = strtoul(argv[1], NULL, 10);
	}

	pthread_t threads[thread_count];
	unsigned int i, param[thread_count];
	int ret;

	// if we send the signals too early some threads might not have registered
	// a signal handler yet
	pthread_barrier_init(&barrier, &attr, thread_count + 1);

	for(i = 0; i < thread_count; i++) {
		param[i] = i;
		ret = pthread_create(threads+i, NULL, thread_func, (void*) &param[i]);
		if (ret) {
			printf("Thread creation failed! error =  %d\n", ret);
			return ret;
		} else printf("Create thread %d\n", i);
	}

	pthread_barrier_wait(&barrier);

	for(i = 0; i < thread_count; i++) {
		printf("Send signal to thread %d\n", i);
		pthread_kill(threads[i], 16);
	}

	sys_msleep(500);

	printf("Wait for all threads to finish\n");
	for(i = 0; i < thread_count; i++) {
		pthread_join(threads[i], NULL);
		printf("Thread %d is done\n", i);
	}

	printf("All done\n");

	return 0;
}
