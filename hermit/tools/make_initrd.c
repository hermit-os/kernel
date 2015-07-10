/*
 * Copyright (c) 2010, Stefan Lankes, RWTH Aachen University
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

#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <stdint.h>

#define INITRD_MAGIC_NUMBER	0x4711
#define MAX_FNAME		128
#define PAGE_SIZE		4096

typedef struct {
	uint32_t magic;
	uint32_t nfiles;
	char mount_point[MAX_FNAME];
} initrd_header_t;

typedef struct {
	uint32_t length;
	uint32_t offset;
	char fname[MAX_FNAME];
} initrd_file_desc_t;

static void print_options(void)
{
	printf("  make_initrd mount_point path name [path name]\n");
	printf("\n");
	printf("    mount_point - mount point of init ram disk, where all file will be mounted.\n");
	printf("    path - path to the file, which will be mounted\n");
	printf("    name - file name, which will be used be the initrd\n");
}

int main(int argc, char **argv)
{
	int i, nfiles = (argc - 2) / 2;
	initrd_header_t	header;
	initrd_file_desc_t* file_desc;
	off_t offset;
	FILE* istream;
	FILE* ostream;

	if ((argc < 4) || (strcmp(argv[1], "-h") == 0)) {
		print_options();
		return 0;
	}

	memset(&header, 0x00, sizeof(initrd_header_t));
	header.magic = INITRD_MAGIC_NUMBER;
	header.nfiles = nfiles;
	strncpy(header.mount_point, argv[1], MAX_FNAME);

	file_desc = (initrd_file_desc_t*) malloc(sizeof(initrd_file_desc_t)*nfiles);
	if (!file_desc) {
		fprintf(stderr, "No enough memory\n");
		return -1;
	}
	memset(file_desc, 0x00, sizeof(initrd_file_desc_t)*nfiles);
	offset = sizeof(initrd_header_t) + nfiles * sizeof(initrd_file_desc_t);

	for(i=0; i<nfiles; i++) {
		strncpy(file_desc[i].fname, argv[3 + i * 2], MAX_FNAME);

		if (offset % PAGE_SIZE)
			offset += PAGE_SIZE - offset % PAGE_SIZE;
		file_desc[i].offset = offset;

		istream = fopen(argv[2 + i * 2], "r");
		if (istream == NULL) {
			fprintf(stderr, "Error: file not found: %s\n", argv[2 + i * 2]);
			return -1;
		}
		fseek(istream, 0, SEEK_END);
		file_desc[i].length = ftell(istream);
		offset += file_desc[i].length;
		fclose(istream);
	}

	ostream = fopen("./initrd.img", "w");
	if (ostream == NULL) {
		fprintf(stderr, "Error: unable to create file\n");
		return -1;
	}

	fwrite(&header, sizeof(initrd_header_t), 1, ostream);
	fwrite(file_desc, sizeof(initrd_file_desc_t), nfiles, ostream);

	for(i=0; i<nfiles; i++) {
		unsigned char *buf = (unsigned char *)malloc(file_desc[i].length);
		size_t curr, len = file_desc[i].length;
	
		istream = fopen(argv[2 + i * 2], "r");

		while (ftell(ostream) < file_desc[i].offset)
			fwrite(buf, 1, sizeof(unsigned char), ostream);

		do {
			curr = fread(buf, 1, len, istream);
			fwrite(buf, 1, len, ostream);
			len -= curr;
		} while(len > 0);

		fclose(istream);
		free(buf);
	}

	fclose(ostream);

	return 0;
}
