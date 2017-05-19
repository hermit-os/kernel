#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <malloc.h>

#ifndef NUM_ITER
#define NUM_ITER    100000
#endif

#ifndef SIZE
#define SIZE    16*1024
#endif 

void* buf;

int main(int argc, char** argv)
{
    /* optionally: insert more useful stuff here */

    for(int i=0; i<NUM_ITER; i++)
    {
        buf = malloc(SIZE*i);
        free(buf);
    }
    malloc_stats();

    return 0;
}


