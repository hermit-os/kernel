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

#define _GNU_SOURCE

#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/tcp.h>
#include <netinet/in.h>
#include <sched.h>
#include <signal.h>
#include <sys/inotify.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <sys/ioctl.h>
#include <net/if.h>

#include "proxy.h"

#define MAX_PATH	255
#define MAX_ARGS	1024
#define INADDR(a, b, c, d) (struct in_addr) { .s_addr = ((((((d) << 8) | (c)) << 8) | (b)) << 8) | (a) }

#define HERMIT_PORT	0x494E
#define HERMIT_IP(isle)	INADDR(192, 168, 28, isle + 2)
#define HERMIT_MAGIC	0x7E317

#define EVENT_SIZE	(sizeof (struct inotify_event))
#define BUF_LEN		(1024 * (EVENT_SIZE + 16))

#if 0
#define PROXY_DEBUG(fmt, ...) fprintf(stderr, fmt, ##__VA_ARGS__);
#else
#define PROXY_DEBUG(fmt, ...) {}
#endif

typedef enum {
  BAREMETAL = 0,
  QEMU,
  UHYVE
} monitor_t;

static monitor_t monitor = BAREMETAL;
static int sobufsize = 131072;
static unsigned int isle_nr = 0;
static unsigned int port = HERMIT_PORT;
static char pidname[] = "/tmp/hpid-XXXXXX";
static char tmpname[] = "/tmp/hermit-XXXXXX";
static char cmdline[MAX_PATH] = "";

extern char **environ;

static void stop_hermit(void);
static void dump_log(void);
static int multi_init(char *path);
static int qemu_init(char *path);

static void qemu_fini(void)
{
	FILE* fp = NULL;

	// try to kill qemu
	if (monitor == QEMU)
		fp = fopen(pidname, "r");
	if (fp) {
		pid_t id = -1;

		int ret = fscanf(fp, "%d", &id);
		if (ret <= 0)
			fprintf(stderr, "Unable to read Qemu's pid\n");
		fclose(fp);
		unlink(pidname);

		if (id >= 0) {
			int ret;

			do {
				ret = kill(id, SIGINT);
				sched_yield();
			} while((ret < 0) && (errno == ESRCH));
		}
	}

	dump_log();
	unlink(tmpname);
}

static void multi_fini(void)
{
	dump_log();
	stop_hermit();
}

static void exit_handler(int sig)
{
	exit(0);
}

static char* get_append_string(void)
{
	char line[2048];
	char* match;
	char* point;

	FILE* fp = fopen("/proc/cpuinfo", "r");
	if (!fp)
		return "-freq0";

	while(fgets(line, 2048, fp)) {
		if ((match = strstr(line, "cpu MHz")) == NULL)
			continue;

		// scan strinf for the next number
		for(; (*match < 0x30) || (*match > 0x39); match++)
			;

		for(point = match; ((*point != '.') && (*point != '\0')); point++)
			;
		*point = '\0';

		snprintf(cmdline, MAX_PATH, "\"-freq%s -proxy\"", match);
		fclose(fp);

		return cmdline;
	}

	return "-freq0";
}

static int env_init(char *path)
{
	char* str;
	struct sigaction sINT, sTERM;

	// define action for SIGINT
	sINT.sa_handler = exit_handler;
	sINT.sa_flags = 0;
	if (sigaction(SIGINT, &sINT, NULL) < 0)
	{
		perror("sigaction");
		exit(1);
	}

	// define action for SIGTERM
	sTERM.sa_handler = exit_handler;
	sTERM.sa_flags = 0;
	if (sigaction(SIGTERM, &sTERM, NULL) < 0)
	{
		perror("sigaction");
		exit(1);
	}

	str = getenv("HERMIT_ISLE");
	if (str)
	{
		if (strncmp(str, "qemu", 4) == 0) {
			monitor = QEMU;
			isle_nr = 0;
		} else if (strncmp(str, "uhyve", 5) == 0) {
			monitor = UHYVE;
			isle_nr = 0;
		} else {
			isle_nr = atoi(str);
			if (isle_nr > 254)
				isle_nr = 0;
		}
	}

	str = getenv("HERMIT_PORT");
	if (str)
	{
		port = atoi(str);
		if ((port == 0) || (port >= UINT16_MAX))
			port = HERMIT_PORT;
	}

	if (monitor == QEMU) {
		atexit(qemu_fini);
		return qemu_init(path);
	} else if (monitor == UHYVE) {
		return uhyve_init(path);
	} else {
		atexit(multi_fini);
		return multi_init(path);
	}
}

static int is_hermit_available(void)
{
	char* line = (char*) malloc(2048);
	size_t n = 2048;
	int ret = 0;
	FILE* file;

	if (!line) {
		fprintf(stderr, "Not enough memory\n");
		exit(1);
	}

	if (monitor == QEMU) {
		file = fopen(tmpname, "r");
		if (!file) {
			PROXY_DEBUG("%s isn't available\n", tmpname);
		}
	} else {
		char logname[MAX_PATH];

		snprintf(logname, MAX_PATH, "/sys/hermit/isle%d/log", isle_nr);
		file = fopen(logname, "r");
	}

	if (!file)
		return 0;

	//PROXY_DEBUG("Open log file\n");

	while(getline(&line, &n, file) > 0) {
		if (strstr(line, "TCP server is listening.") != NULL) {
			ret = 1;
			break;
		}
		//PROXY_DEBUG("%s\n", line);
	}

	fclose(file);
	free(line);

	return ret;
}

// wait until HermitCore is sucessfully booted
static void wait_hermit_available(void)
{
	char buffer[BUF_LEN];
	int wd;

	if (is_hermit_available())
		return;

	int fd = inotify_init();
	if (fd < 0) {
		perror( "inotify_init" );
		exit(1);
	}

	if (monitor == QEMU)
		wd = inotify_add_watch(fd, "/tmp", IN_MODIFY|IN_CREATE);
	else
		wd = inotify_add_watch(fd, "/sys/hermit", IN_MODIFY|IN_CREATE);

	if (wd < 0) {
		perror("inotify_add_watch");
		exit(1);
	}

	while(1) {
		int length = read(fd, buffer, BUF_LEN);

		if (length < 0) {
			perror("read");
			break;
		}

		if (length != 0 && is_hermit_available())
			break;
	}

	//printf("HermitCore is available\n");
	if (inotify_rm_watch(fd, wd) < 0) {
		perror("inotify_rm_watch");
		exit(1);
	}
	close(fd);
}

static int qemu_init(char *path)
{
	int kvm, i = 0;
	char* str;
	char loader_path[MAX_PATH];
	char hostfwd[MAX_PATH];
	char monitor_str[MAX_PATH];
	char chardev_file[MAX_PATH];
	char port_str[MAX_PATH];
	pid_t qemu_pid;
	char* qemu_str = "qemu-system-x86_64";
	char* qemu_argv[] = {qemu_str, "-daemonize", "-display", "none", "-smp", "1", "-m", "2G", "-pidfile", pidname, "-net", "nic,model=rtl8139", "-net", hostfwd, "-chardev", chardev_file, "-device", "pci-serial,chardev=gnc0", "-kernel", loader_path, "-initrd", path, "-append", get_append_string(), NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL};

	str = getenv("HERMIT_CPUS");
	if (str)
		qemu_argv[5] = str;

	str = getenv("HERMIT_MEM");
	if (str)
		qemu_argv[7] = str;

	str = getenv("HERMIT_QEMU");
	if (str)
		qemu_argv[0] = qemu_str = str;

	snprintf(hostfwd, MAX_PATH, "user,hostfwd=tcp:127.0.0.1:%u-:%u", port, port);
	snprintf(monitor_str, MAX_PATH, "telnet:127.0.0.1:%d,server,nowait", port+1);

	if (mkstemp(pidname) < 0)
	{
		perror("mkstemp");
		exit(1);
	}

	if (mkstemp(tmpname) < 0)
	{
		perror("mkstemp");
		exit(1);
	}
	snprintf(chardev_file, MAX_PATH, "file,id=gnc0,path=%s", tmpname);

	if (readlink("/proc/self/exe", loader_path, MAX_PATH) < 0)
	{
		perror("readlink");
		exit(1);
	}
	str = strstr(loader_path, "proxy");
	strncpy(str, "ldhermit.elf", MAX_PATH-strlen(loader_path)+5);

	str = getenv("HERMIT_APP_PORT");
	if (str)
	{
		int app_port = atoi(str);

		if (app_port > 0) {
			for(; qemu_argv[i] != NULL; i++)
				;

			snprintf(port_str, MAX_PATH, "tcp:%u::%u", app_port, app_port);

			qemu_argv[i] = "-redir";
			qemu_argv[i+1] = port_str;
		}
	}

	str = getenv("HERMIT_KVM");
	if (str && (strcmp(str, "0") == 0))
		kvm = 0;
	else
		kvm = 1;

	if (kvm)
	{
		for(; qemu_argv[i] != NULL; i++)
			;

		qemu_argv[i] = "-machine";
		qemu_argv[i+1] = "accel=kvm";
		qemu_argv[i+2] = "-cpu";
		qemu_argv[i+3] = "host";
	} /*else {
		for(; qemu_argv[i] != NULL; i++)
			;

		qemu_argv[i] = "-cpu";
		qemu_argv[i+1] = "SandyBridge";
	}*/

	str = getenv("HERMIT_MONITOR");
	if (str && (strcmp(str, "0") != 0))
	{
		for(; qemu_argv[i] != NULL; i++)
			;

		qemu_argv[i] = "-monitor";
		qemu_argv[i+1] = monitor_str;
	}

	str = getenv("HERMIT_DEBUG");
	if (str && (strcmp(str, "0") != 0))
	{
		for(; qemu_argv[i] != NULL; i++)
			;

		// add flag to start gdbserver on TCP port 1234
		qemu_argv[i] = "-s";
	}

	str = getenv("HERMIT_CAPTURE_NET");
	if (str && (strcmp(str, "0") != 0))
	{
		for(; qemu_argv[i] != NULL; i++)
			;

		// add flags to capture the network traffic
		qemu_argv[i] = "-net";
		qemu_argv[i+1] = "dump";
	}

	str = getenv("HERMIT_VERBOSE");
	if (str && (strcmp(str, "0") != 0))
	{
		printf("qemu startup command: ");

		for(i=0; qemu_argv[i] != NULL; i++)
			printf("%s ", qemu_argv[i]);
		printf("\n");
		fflush(stdout);
	}

	qemu_pid = fork();
	if (qemu_pid == 0)
	{
		execvp(qemu_str, qemu_argv);

		fprintf(stderr, "Didn't find qemu\n");
		exit(1);
	} else if (qemu_pid < 0) {
		perror("fork");
		exit(1);
	}

	PROXY_DEBUG("Create VM with pid %d\n", qemu_pid);

	// move the parent process to the end of the queue
	// => child would be scheduled next
	sched_yield();

	// wait until HermitCore is sucessfully booted
	wait_hermit_available();

	PROXY_DEBUG("VM is available\n");

	return 0;
}

static int multi_init(char *path)
{
	int ret;
	char* str;
	FILE* file;
	char isle_path[MAX_PATH];
	char* result;

	// set path to temporary file
	snprintf(isle_path, MAX_PATH, "/sys/hermit/isle%d/path", isle_nr);
	file = fopen(isle_path, "w");
	if (!file) {
		perror("fopen");
		exit(1);
	}

	fprintf(file, "%s", path);
	fclose(file);

	// start application
	snprintf(isle_path, MAX_PATH, "/sys/hermit/isle%d/cpus", isle_nr);
	file = fopen(isle_path, "w");
	if (!file) {
		perror("fopen");
		exit(1);
	}

	str = getenv("HERMIT_CPUS");
	if (str)
		fprintf(file, "%s", str);
	else
		fprintf(file, "%s", "1");

	fclose(file);

	// check result
	file = fopen(isle_path, "r");
	if (!file) {
		perror("fopen");
		exit(1);
	}

	result = NULL;
	ret = fscanf(file, "%ms", &result);

	fclose(file);

	if (ret <= 0) {
		fprintf(stderr, "Unable to check the boot process!\n");
		exit(1);
	}

	if (strcmp(result, "-1") == 0) {
		free(result);
		fprintf(stderr, "Unable to boot cores %s\n", str ? str : "1");
		exit(1);
	}

	free(result);

	// wait until HermitCore is sucessfully booted
	//wait_hermit_available();

	return 0;
}

static void dump_log(void)
{
	char* str = getenv("HERMIT_VERBOSE");
	FILE* file;
	char line[2048];

	if (!(str && (strcmp(str, "0") != 0)))
		return;

	if (monitor == BAREMETAL)
	{
		char isle_path[MAX_PATH];

		snprintf(isle_path, MAX_PATH, "/sys/hermit/isle%d/log", isle_nr);
		file = fopen(isle_path, "r");
	} else file = fopen(tmpname, "r");

	if (!file) {
		perror("fopen");
		return;
	}

	puts("\nDump kernel log:");
	puts("================\n");

	while(fgets(line, 2048, file)) {
		printf("%s", line);
	}

	fclose(file);
}

static void stop_hermit(void)
{
	FILE* file;
	char isle_path[MAX_PATH];

	fflush(stdout);
	fflush(stderr);

	snprintf(isle_path, MAX_PATH, "/sys/hermit/isle%d/cpus", isle_nr);

	file = fopen(isle_path, "w");
	if (!file) {
		perror("fopen");
		return;
	}

	fprintf(file, "-1");

	fclose(file);
}

/*
 * in principle, HermitCore forwards basic system calls to
 * this proxy, which mapped these call to Linux system calls.
 */
int handle_syscalls(int s)
{
	int sysnr;
	ssize_t sret;
	size_t j;

	while(1)
	{
		j = 0;
		while(j < sizeof(sysnr)) {
			sret = read(s, ((char*)&sysnr)+j, sizeof(sysnr)-j);
			if (sret < 0)
				goto out;
			j += sret;
		}

		switch(sysnr)
		{
		case __HERMIT_exit: {
			int arg = 0;

			j = 0;
			while(j < sizeof(arg)) {
				sret = read(s, ((char*)&arg)+j, sizeof(arg)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}
			close(s);

			// already called by fini_env
			//dump_log();
			//stop_hermit();

			if (arg == -14)
				fprintf(stderr, "Did HermitCore receive an exception?\n");
			exit(arg);
			break;
		}
		case __HERMIT_write: {
			int fd;
			size_t len;
			char* buff;

			j = 0;
			while (j < sizeof(fd)) {
				sret = read(s, ((char*)&fd)+j, sizeof(fd)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			j = 0;
			while (j < sizeof(len)) {
				sret = read(s, ((char*)&len)+j, sizeof(len)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			buff = malloc(len);
			if (!buff) {
				fprintf(stderr,"Proxy: not enough memory");
				return 1;
			}

			j=0;
			while(j < len)
			{
				sret = read(s, buff+j, len-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			if (fd > 2) {
				sret = write(fd, buff, len);
				j = 0;
				while(j < sizeof(sret))
				{
					int l = write(s, ((char*)&sret)+j, sizeof(sret)-j);
					if (l < 0)
						goto out;
					j += l;
				}
			} else {
				j = 0;
				while(j < len)
				{
					sret = write(fd, buff+j, len-j);
					if (sret < 0)
						goto out;
					j += sret;
				}
			}

			free(buff);
			break;
		}
		case __HERMIT_open: {
			size_t len;
			char* fname;
			int flags, mode, ret;

			j = 0;
			while (j < sizeof(len))
			{
				sret = read(s, ((char*)&len)+j, sizeof(len)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			fname = malloc(len);
			if (!fname)
				goto out;

			j = 0;
			while (j < len)
			{
				sret = read(s, fname+j, len-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			j = 0;
			while (j < sizeof(flags))
			{
				sret = read(s, ((char*)&flags)+j, sizeof(flags)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			j = 0;
			while (j < sizeof(mode))
			{
				sret = read(s, ((char*)&mode)+j, sizeof(mode)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			//printf("flags 0x%x, mode 0x%x\n", flags, mode);

			ret = open(fname, flags, mode);
			j = 0;
			while(j < sizeof(ret))
			{
				sret = write(s, ((char*)&ret)+j, sizeof(ret)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			free(fname);
			break;
		}
		case __HERMIT_close: {
			int fd, ret;

			j = 0;
			while(j < sizeof(fd))
			{
				sret = read(s, ((char*)&fd)+j, sizeof(fd)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			if (fd > 2)
				ret = close(fd);
			else
				ret = 0;

			j = 0;
			while (j < sizeof(ret))
			{
				sret = write(s, ((char*)&ret)+j, sizeof(ret)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}
			break;
		}
		case __HERMIT_read: {
			int fd, flag;
			size_t len;
			ssize_t sj;
			char* buff;

			j = 0;
			while(j < sizeof(fd))
			{
				sret = read(s, ((char*)&fd)+j, sizeof(fd)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			j = 0;
			while(j < sizeof(len))
			{
				sret = read(s, ((char*)&len)+j, sizeof(len)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			buff = malloc(len);
			if (!buff)
				goto out;

			sj = read(fd, buff, len);

			flag = 0;
			setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (char *) &flag, sizeof(int));

			j = 0;
			while (j < sizeof(sj))
			{
				sret = write(s, ((char*)&sj)+j, sizeof(sj)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			if (sj > 0)
			{
				size_t i = 0;

				while (i < sj)
				{
					sret = write(s, buff+i, sj-i);
					if (sret < 0)
						goto out;

					i += sret;
				}
			}

			flag = 1;
			setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (char *) &flag, sizeof(int));

			free(buff);
			break;
		}
		case __HERMIT_lseek: {
			int fd, whence;
			off_t offset;

			j = 0;
			while (j < sizeof(fd))
			{
				sret = read(s, ((char*)&fd)+j, sizeof(fd)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			j = 0;
			while (j < sizeof(offset))
			{
				sret = read(s, ((char*)&offset)+j, sizeof(offset)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			j = 0;
			while (j < sizeof(whence))
			{
				sret = read(s, ((char*)&whence)+j, sizeof(whence)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}

			offset = lseek(fd, offset, whence);

			j = 0;
			while (j < sizeof(offset))
			{
				sret = write(s, ((char*)&offset)+j, sizeof(offset)-j);
				if (sret < 0)
					goto out;
				j += sret;
			}
			break;
		}
		default:
			fprintf(stderr, "Proxy: invalid syscall number %d, errno %d, ret %zd\n", sysnr, errno, sret);
			close(s);
			exit(1);
			break;
		}
	}

out:
	perror("Proxy -- communication error");

	return 1;
}

int socket_loop(int argc, char **argv)
{
	int i, j, ret, s;
	int32_t magic = HERMIT_MAGIC;
	struct sockaddr_in serv_name;

#if 0
		// check if mmnif interface is available
		if (!qemu) {
			struct ifreq ethreq;

			memset(&ethreq, 0, sizeof(ethreq));
			strncpy(ethreq.ifr_name, "mmnif", IFNAMSIZ);

			while(1) {
				/* this socket doesn't really matter, we just need a descriptor
				 * to perform the ioctl on */
				s = socket(PF_INET, SOCK_STREAM, IPPROTO_TCP);
				ioctl(s, SIOCGIFFLAGS, &ethreq);
				close(s);

				if (ethreq.ifr_flags & (IFF_UP|IFF_RUNNING))
					break;
			}
			sched_yield();
		}
#endif

		/* create a socket */
		s = socket(PF_INET, SOCK_STREAM, 0);
		if (s < 0)
		{
			perror("Proxy: socket creation error");
			exit(1);
		}

		setsockopt(s, SOL_SOCKET, SO_RCVBUF, (char *) &sobufsize, sizeof(sobufsize));
		setsockopt(s, SOL_SOCKET, SO_SNDBUF, (char *) &sobufsize, sizeof(sobufsize));
		i = 1;
		setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (char *) &i, sizeof(i));
		i = 0;
		setsockopt(s, SOL_SOCKET, SO_KEEPALIVE, (char *) &i, sizeof(i));

		/* server address  */
		memset((char *) &serv_name, 0x00, sizeof(serv_name));
		serv_name.sin_family = AF_INET;
		if (monitor == QEMU)
			serv_name.sin_addr = INADDR(127, 0, 0, 1);
		else
			serv_name.sin_addr = HERMIT_IP(isle_nr);
		serv_name.sin_port = htons(port);

		i = 0;
	retry:
		ret = connect(s, (struct sockaddr*)&serv_name, sizeof(serv_name));
		if (ret < 0)
		{
			i++;
			if (i <= 10) {
				usleep(10000);
				goto retry;
			}
			perror("Proxy -- connection error");
			close(s);
			exit(1);
		}

		ret = write(s, &magic, sizeof(magic));
		if (ret < 0)
			goto out;

		// forward program arguments to HermitCore
		// argv[0] is path of this proxy so we strip it

		argv++;
		argc--;

		ret = write(s, &argc, sizeof(argc));
		if (ret < 0)
			goto out;

		for(i=0; i<argc; i++)
		{
			int len = strlen(argv[i])+1;

			j = 0;
			while (j < sizeof(len))
			{
				ret = write(s, ((char*)&len)+j, sizeof(len)-j);
				if (ret < 0)
					goto out;
				j += ret;
			}

			j = 0;
			while (j < len)
			{
				ret = write(s, argv[i]+j, len-j);
				if (ret < 0)
					goto out;
				j += ret;
			}
		}

		// send environment
		i = 0;
		while(environ[i])
			i++;

		ret = write(s, &i, sizeof(i));
		if (ret < 0)
			goto out;

		for(i=0; environ[i] ;i++)
		{
			int len = strlen(environ[i])+1;

			j = 0;
			while (j < sizeof(len))
			{
				ret = write(s, ((char*)&len)+j, sizeof(len)-j);
				if (ret < 0)
					goto out;
				j += ret;
			}

			j = 0;
			while (j < len)
			{
				ret = write(s, environ[i]+j, len-j);
				if (ret < 0)
					goto out;
				j += ret;
			}
		}

		ret = handle_syscalls(s);

		close(s);

		return ret;

	out:
		perror("Proxy -- communication error");
		close(s);
		return 1;
}

int main(int argc, char **argv)
{
	int ret;

	ret = env_init(argv[1]);
	if (ret)
		return ret;


	switch(monitor) {
	case UHYVE:
		return uhyve_loop();

	case BAREMETAL:
	case QEMU:
		return socket_loop(argc, argv);

	default:
		perror("Unknown monitor");
	}

	return 1;
}
