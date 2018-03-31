#ifndef __UHYVE_NET_H__
#define __UHYVE_NET_H__

#include <linux/kvm.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
#include <errno.h>
#include <unistd.h>

#include <sys/select.h>
#include <sys/stat.h>

/* network interface */
#include <sys/socket.h>
#include <linux/if.h>
#include <linux/if_tun.h>
#include <fcntl.h>
#include <sys/ioctl.h>
#include <err.h>

extern int netfd;

// UHYVE_PORT_NETINFO
typedef struct {
	/* OUT */
	char mac_str[18];
} __attribute__((packed)) uhyve_netinfo_t;

// UHYVE_PORT_NETWRITE
typedef struct {
	/* IN */
	const void* data;
	size_t len;
	/* OUT */
	int ret;
} __attribute__((packed)) uhyve_netwrite_t;

// UHYVE_PORT_NETREAD
typedef struct {
        /* IN */
	void* data;
        /* IN / OUT */
	size_t len;
	/* OUT */
	int ret;
} __attribute__((packed)) uhyve_netread_t;

// UHYVE_PORT_NETSTAT
typedef struct {
        /* IN */
        int status;
} __attribute__((packed)) uhyve_netstat_t;

int uhyve_net_init(const char *hermit_netif);
char* uhyve_get_mac(void);

#endif
