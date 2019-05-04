/*
 * Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
 * http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
 * http://opensource.org/licenses/MIT>, at your option. This file may not be
 * copied, modified, or distributed except according to those terms.
 */

#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>

#define N	255

int main(int argc, char** argv)
{
	int i, random;
	FILE* file;

	printf("Hello World!!!\n");
	//for(i=0; environ[i]; i++)
	//	printf("environ[%d] = %s\n", i, environ[i]);
	for(i=0; i<argc; i++)
		printf("argv[%d] = %s\n", i, argv[i]);

	file = fopen("/etc/hostname", "r");
	if (file)
	{
		char fname[N] = "";

		fscanf(file, "%s", fname);
		printf("Hostname: %s\n", fname);
		fclose(file);
	} else fprintf(stderr, "Unable to open file /etc/hostname\n");

	file = fopen("/tmp/test.txt", "w");
	if (file)
	{
		fprintf(file, "Hello World!!!\n");
		fclose(file);
	} else fprintf(stderr, "Unable to open file /tmp/test.txt\n");

	return 0;
}
