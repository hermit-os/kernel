#include <stdio.h>
#include <pthread.h>

#define MAX_THREADS 2

void* thread_func(void* arg)
{
	int id = *((int*) arg);

	printf("Hello Thread!!! id = %d\n", id);

	return 0;
}

int main(int argc, char** argv)
{
	pthread_t threads[MAX_THREADS];
	int i, ret, param[MAX_THREADS];

	for(i=0; i<MAX_THREADS; i++) {
		param[i] = i;
		ret = pthread_create(threads+i, NULL, thread_func, param+i);
		if (ret) {
			printf("Thread creation failed! error =  %d\n", ret);
			return ret;
		} else printf("Create thread %d\n", i);
	}

	/* wait until all threads have terminated */
	for(i=0; i<MAX_THREADS; i++)
		pthread_join(threads[i], NULL);	

	return 0;
}
