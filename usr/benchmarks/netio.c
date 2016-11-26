/*
 * Copyright (c) 2011, Stefan Lankes, RWTH Aachen University
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
 * DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR AN
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <unistd.h>
#include <string.h>
#include <time.h>
#include <errno.h>

/*
 * This implements a netio server and client (only TCP version).
 * The client sends a command word (4 bytes) then a data length word (4 bytes).
 * If the command is "receive", the server is to consume "data length" bytes into
 * a circular buffer until the first byte is non-zero, then it is to consume
 * another command/data pair.
 * If the command is "send", the server is to send "data length" bytes from a circular
 * buffer with the first byte being zero, until "some time" (6 seconds in the
 * current netio131.zip download) has passed and then send one final buffer with
 * the first byte being non-zero. Then it is to consume another command/data pair.
 */

/* See http://www.nwlab.net/art/netio/netio.html to get the netio tool */

#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <netdb.h>

typedef struct
{
	uint32_t cmd;
	uint32_t data;
} CONTROL;

#define CMD_QUIT  0
#define CMD_C2S   1
#define CMD_S2C   2
#define CMD_RES   3

#define CTLSIZE sizeof(CONTROL)
#define DEFAULTPORT 0x494F
#define TMAXSIZE 65536

static int tSizes[] = {1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32767};
static size_t ntSizes = sizeof(tSizes) / sizeof(int);
static int nPort = DEFAULTPORT;
static const int sobufsize = 131072;
static struct in_addr addr_local;
static struct in_addr addr_server;

extern unsigned int get_cpufreq(void);

inline static unsigned long long rdtsc(void)
{
	unsigned long lo, hi;
	asm volatile ("rdtsc" : "=a"(lo), "=d"(hi) :: "memory");
	return ((unsigned long long) hi << 32ULL | (unsigned long long) lo);
}

static int send_data(int socket, void *buffer, size_t size, int flags)
{
	ssize_t rc = send(socket, buffer, size, flags);

	if (rc < 0)
	{
		printf("send failed: %d\n", errno);
		return -1;
	}

	if (rc != size)
		return 1;

	return 0;
}

static int recv_data(int socket, void *buffer, size_t size, int flags)
{
	ssize_t rc = recv(socket, buffer, size, flags);

	if (rc < 0) {
		printf("recv failed: %d\n", errno);
		return -1;
	}

	if (rc != size)
		return 1;

	return 0;
}

static char *InitBuffer(size_t nSize)
{
	char *cBuffer = malloc(nSize);

	memset(cBuffer, 0xFF, nSize);
	cBuffer[0] = 0;

	return cBuffer;
}

static char *PacketSize(int nSize)
{
	static char szBuffer[64];

	if ((nSize % 1024) == 0 || (nSize % 1024) == 1023)
		sprintf(szBuffer, "%2dk", (nSize + 512) / 1024);
	else
		sprintf(szBuffer, "%d", nSize);

	return szBuffer;
}

static int TCPServer(void)
{
	char *cBuffer;
	CONTROL ctl;
	uint64_t nData;
	struct sockaddr_in sa_server, sa_client;
	int server, client;
	socklen_t length;
	struct timeval tv;
	fd_set fds;
	int rc;
	int nByte;
	int err;
	uint64_t start, end;
	uint32_t freq = get_cpufreq(); /* in MHz */

	if ((cBuffer = InitBuffer(TMAXSIZE)) == NULL) {
    		printf("Netio: Not enough memory\n");
		return -1;
	}

	if ((server = socket(PF_INET, SOCK_STREAM, 0)) < 0) {
		printf("socket failed: %d\n", server);
 		free(cBuffer);
 		return -1;
	}

	setsockopt(server, SOL_SOCKET, SO_RCVBUF, (char *) &sobufsize, sizeof(sobufsize));
	setsockopt(server, SOL_SOCKET, SO_SNDBUF, (char *) &sobufsize, sizeof(sobufsize));

	memset((char *) &sa_server, 0x00, sizeof(sa_server));
	sa_server.sin_family = AF_INET;
	sa_server.sin_port = htons(nPort);
	sa_server.sin_addr = addr_local;

	if ((err = bind(server, (struct sockaddr *) &sa_server, sizeof(sa_server))) < 0)
	{
		printf("bind failed: %d\n", errno);
		close(server);
		free(cBuffer);
		return -1;
	}

	if ((err = listen(server, 2)) != 0)
	{
		printf("listen failed: %d\n", errno);
		close(server);
		free(cBuffer);
		return -1;
	}

	for (;;)
	{
		printf("TCP server listening.\n");

		FD_ZERO(&fds);
		FD_SET(server, &fds);
		tv.tv_sec  = 3600;
		tv.tv_usec = 0;

		if ((rc = select(FD_SETSIZE, &fds, 0, 0, &tv)) < 0)
		{
			printf("select failed: %d\n", errno);
			break;
		}

		if (rc == 0 || FD_ISSET(server, &fds) == 0)
			continue;
		length = sizeof(sa_client);

		if ((client = accept(server, (struct sockaddr *) &sa_client, &length)) < 0) {
			printf("accept faild: %d\n", errno);
			continue;
		}

		setsockopt(client, SOL_SOCKET, SO_RCVBUF, (char *) &sobufsize, sizeof(sobufsize));
		setsockopt(client, SOL_SOCKET, SO_SNDBUF, (char *) &sobufsize, sizeof(sobufsize));

		printf("TCP connection established ... ");

		for (;;)
		{
			if (recv_data(client, (void *) &ctl, CTLSIZE, 0))
				break;

			ctl.cmd = ntohl(ctl.cmd);
			ctl.data = ntohl(ctl.data);

			if (ctl.cmd == CMD_C2S)
			{
				start = rdtsc();

				printf("\nReceiving from client, packet size %s ... \n", PacketSize(ctl.data));
				cBuffer[0] = 0;
				nData = 0;

				do {
					for (nByte = 0; nByte < ctl.data; )
					{
						rc = recv(client, cBuffer + nByte, ctl.data - nByte, 0);

						if (rc < 0)
						{
							printf("recv failed: %d\n", errno);
							break;
						}

						if (rc > 0)
							nByte += rc;
					}

					nData += ctl.data;
				} while (cBuffer[0] == 0 && rc > 0);

				end = rdtsc();
				printf("Time to receive %llu bytes: %llu nsec (ticks %llu)\n", nData, ((end-start)*1000ULL)/freq, end-start);
			} else if (ctl.cmd == CMD_S2C) {
				start = rdtsc();

				printf("\nSending to client, packet size %s ... \n", PacketSize(ctl.data));
				cBuffer[0] = 0;
				nData = 0;

				do
				{
					//GenerateRandomData(cBuffer, ctl.data);

					for (nByte = 0; nByte < ctl.data; )
					{
						rc = send(client, cBuffer + nByte, ctl.data - nByte, 0);

						if (rc < 0)
						{
							printf("send failed: %d\n", errno);
							break;
						}

						if (rc > 0)
							nByte += rc;
					}

					nData += ctl.data;
					end = rdtsc();
				} while((end-start)/freq < 6000000ULL /* = 6s */);

				cBuffer[0] = 1;

				if (send_data(client, cBuffer, ctl.data, 0))
					break;

				end = rdtsc();
				printf("Time to send %llu bytes: %llu nsec (ticks %llu)\n", nData, ((end-start)*1000ULL)/freq, end-start);
			} else /* quit */
				break;
		}

		printf("\nDone.\n");

		close(client);

		if (rc < 0)
			break;
	}

	close(server);
	free(cBuffer);

	return 0;
}

int TCP_Bench(void)
{
	char *cBuffer;
	CONTROL ctl;
	uint64_t nData;
	int i;
	struct sockaddr_in sa_server;
	int server;
	int rc, err;
	int nByte;
	uint64_t start, end;
	uint32_t freq = get_cpufreq(); /* in MHz */

	if ((cBuffer = InitBuffer(TMAXSIZE)) == NULL)
	{
		printf("Netio: Not enough memory\n");
		return -1;
	}

	if ((server = socket(PF_INET, SOCK_STREAM, 0)) < 0)
	{
		printf("socket failed: %d\n", errno);
		free(cBuffer);
		return -2;
	}

	setsockopt(server, SOL_SOCKET, SO_RCVBUF, (char *) &sobufsize, sizeof(sobufsize));
	setsockopt(server, SOL_SOCKET, SO_SNDBUF, (char *) &sobufsize, sizeof(sobufsize));

	sa_server.sin_family = AF_INET;
	sa_server.sin_port = htons(nPort);
	sa_server.sin_addr = addr_server;

	if ((err = connect(server, (struct sockaddr *) &sa_server, sizeof(sa_server))) < 0)
	{
		printf("connect failed: %d\n", errno);
		close(server);
		free(cBuffer);
		return -2;
	}

	printf("\nTCP connection established.\n");

	for (i = 0; i < ntSizes; i++)
	{
		printf("Packet size %s bytes: ", PacketSize(tSizes[i]));

		/* tell the server we will send it data now */

		ctl.cmd = htonl(CMD_C2S);
		ctl.data = htonl(tSizes[i]);

		if (send_data(server, (void *) &ctl, CTLSIZE, 0))
			break;

		/* 1 - Tx test */

		start = rdtsc();
		nData = 0;
		cBuffer[0] = 0;

		do
		{
			//GenerateRandomData(cBuffer, tSizes[i]);

			for (nByte = 0; nByte < tSizes[i]; )
			{
				rc = send(server, cBuffer + nByte, tSizes[i] - nByte, 0);

				if (rc < 0)
				{
					printf("send failed: %d\n", errno);
					return -1;
				}

				if (rc > 0)
					nByte += rc;
			}

			nData += tSizes[i];
			end = rdtsc();
		} while((end-start)/freq < 6000000ULL /* = 6s */);

		printf("%llu/100 MBytes/s", ((100ULL*nData)/(1024ULL*1024ULL))/((end-start)/(1000000ULL*freq)));

		printf(" Tx, ");

		cBuffer[0] = 1;

		if (send_data(server, cBuffer, tSizes[i], 0))
			break;

		/* tell the server we expect him to send us data now */

		ctl.cmd = htonl(CMD_S2C);
		ctl.data = htonl(tSizes[i]);

		if (send_data(server, (void *) &ctl, CTLSIZE, 0))
			break;

		/* 2 - Rx test */

		start = rdtsc();
		nData = 0;
		cBuffer[0] = 0;
		rc = 0;

		do
		{
			for (nByte = 0; nByte < tSizes[i]; )
			{
				rc = recv(server, cBuffer + nByte, tSizes[i] - nByte, 0);

				if (rc < 0)
				{
					printf("recv failed: %d\n", errno);
					return -1;
				}

				if (rc > 0)
					nByte += rc;
			}

			nData += tSizes[i];
		} while (cBuffer[0] == 0 && rc > 0);

		end = rdtsc();
		printf("%llu/100 MBytes/s", ((100ULL*nData)/(1024ULL*1024ULL))/((end-start)/(1000000ULL*freq)));

		printf(" Rx.\n");
	}

	ctl.cmd = htonl(CMD_QUIT);
	ctl.data = 0;

	send_data(server, (void *) &ctl, CTLSIZE, 0);

	printf("Done.\n");

	close(server);
	free(cBuffer);

	return 0;
}

int main(int argc, char** argv)
{
	int err = 0;

	addr_local.s_addr = INADDR_ANY;
	//addr_server.s_addr = inet_addr("192.168.28.254");
	addr_server.s_addr = inet_addr("192.168.28.1");

	err = TCPServer();

	return err;
}
