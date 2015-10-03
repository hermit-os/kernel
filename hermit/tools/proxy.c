/*
 * Copyright (c) 2015, Stefan Lankes, RWTH Aachen University
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

#include <unistd.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <fcntl.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

#define HERMIT_PORT     0x494F
#define HERMIT_MAGIC    0x7E317

#define __HERMIT_exit	0
#define __HERMIT_write	1
#define __HERMIT_open	2
#define __HERMIT_close	3
#define __HERMIT_read	4

static char saddr[16] = "192.168.28.2";
static int sobufsize = 131072;

extern char hermit_app[];
extern unsigned app_size;

/*
 * in principle, HermitCore forwards basic system calls to
 * this proxy, which mapped these call to Linux system calls.
 */
int handle_syscalls(int s)
{
	int ret;
	int sysnr;

	while(1)
	{
		ret = read(s, &sysnr, sizeof(sysnr));
		if (ret < 0)
			goto out;

		switch(sysnr)
		{
		case __HERMIT_exit: {
			int arg;

			ret = read(s, &arg, sizeof(arg));
			if (ret < 0)
				goto out;
			close(s);
			exit(arg);
			break;
		}
		case __HERMIT_write: {
			int fd;
			size_t j, len;
			char* buff;

			ret = read(s, &fd, sizeof(fd));
			if (ret < 0)
				goto out;
			ret = read(s, &len, sizeof(len));
			if (ret < 0)
				goto out;

			buff = malloc(len);
			if (!buff) {
				fprintf(stderr,"Proxy: not enough memory");
				return 1;
			}

			j=0;
			while(j < len)
			{
				ret = read(s, buff+j, len-j);
				if (ret < 0)
					goto out;
				j += len;
			}

			j=0;
			while(j < len)
			{
				ret = write(fd, buff+j, len-j);
				if (ret < 0)
					goto out;
				j += len;
			}

			free(buff);
			break;
		}
		case __HERMIT_close: {
			int fd;

			ret = read(s, &fd, sizeof(fd));
			if (ret < 0)
				goto out;

			if (fd > 2)
				ret = close(fd);
			else
				ret = 0;

			ret = write(s, &ret, sizeof(ret));
			if (ret < 0)
				goto out;
			break;
		}
		default:
			fprintf(stderr, "Proxy: invalid syscall number %d\n", sysnr);
			break;
		}
	}

out:
	perror("Proxy: communication error");

	return 1;
}

int main(int argc, char **argv)
{
	int i, j, ret, s;
	int32_t magic = HERMIT_MAGIC;
	struct sockaddr_in serv_name;

	/* create a socket */
	s = socket(PF_INET, SOCK_STREAM, 0);
	if (s < 0)
	{
		perror("Proxy: socket creation error");
		exit(1);
	}

	setsockopt(s, SOL_SOCKET, SO_RCVBUF, (char *) &sobufsize, sizeof(sobufsize));
        setsockopt(s, SOL_SOCKET, SO_SNDBUF, (char *) &sobufsize, sizeof(sobufsize));

	/* server address  */
	memset((char *) &serv_name, 0x00, sizeof(serv_name));
	serv_name.sin_family = AF_INET;
	serv_name.sin_addr.s_addr = inet_addr(saddr);
	serv_name.sin_port = htons(HERMIT_PORT);

	ret = connect(s, (struct sockaddr*)&serv_name, sizeof(serv_name));
	if (ret < 0)
	{
		perror("Proxy: connection error");
		close(s);
		exit(1);
	}

	ret = write(s, &magic, sizeof(magic));
	if (ret < 0)
		goto out;

	// froward program arguments to HermitCore
	ret = write(s, &argc, sizeof(argc));
	if (ret < 0)
		goto out;

	for(i=0; i<argc; i++)
	{
		int len = strlen(argv[i])+1;

		ret = write(s, &len, sizeof(len));
		if (ret < 0)
			goto out;

		j = 0;
		while(j < len)
		{
			ret = write(s, argv[i]+j, len-j);
			if (ret < 0)
				goto out;
			j += ret;
		}
	}

	// send length of the elf file to HermitCore
	ret = write(s, &app_size, sizeof(app_size));
	if (ret < 0)
		goto out;

	// send the executable to HermitCore
	j = 0;
	while(j < app_size)
	{
		ret = write(s, hermit_app+j, app_size-j);
		if (ret < 0)
			goto out;
		j += ret;
	}

	ret = handle_syscalls(s);

	close(s);

	return ret;

out:
	perror("Proxy: communication error");
	close(s);
	return 1;
}
