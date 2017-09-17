/* Copyright (c) 2015, IBM
 * Author(s): Dan Williams <djwillia@us.ibm.com>
 *            Ricardo Koller <kollerr@us.ibm.com>
 * Copyright (c) 2017, RWTH Aachen University
 * Author(s): Tim van de Kamp <tim.van.de.kamp@rwth-aachen.de>
 *
 * Permission to use, copy, modify, and/or distribute this software
 * for any purpose with or without fee is hereby granted, provided
 * that the above copyright notice and this permission notice appear
 * in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL
 * WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
 * AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR
 * CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
 * OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
 * NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
 * CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/* We used several existing projects as guides
 * kvmtest.c: http://lwn.net/Articles/658512/
 * lkvm: http://github.com/clearlinux/kvmtool
 */

/*
 * 15.1.2017: extend original version (https://github.com/Solo5/solo5)
 *            for HermitCore
 */

#include "uhyve-net.h"
#include <ctype.h>

/* TODO: create an array or equal for more then one netif */
static uhyve_netinfo_t netinfo;

//-------------------------------------- ATTACH LINUX TAP -----------------------------------------//
int attach_linux_tap(const char *dev)
{
	struct ifreq ifr;
	int fd, err;

	// @<number> indicates a pre-existing open fd onto the correct device.
	if (dev[0] == '@') {
		fd = atoi(&dev[1]);

		if (fcntl(fd, F_SETFL, O_NONBLOCK) == -1)
			return -1;
		return fd;
	}

	fd = open("/dev/net/tun", O_RDWR | O_NONBLOCK);

	// Initialize interface request for TAP interface
	memset(&ifr, 0x00, sizeof(ifr));

	ifr.ifr_flags = IFF_TAP | IFF_NO_PI;
	if (strlen(dev) > IFNAMSIZ) {
		errno = EINVAL;
		return -1;
	}
	strncpy(ifr.ifr_name, dev, IFNAMSIZ);

	// Try to create OR attach to an existing device. The Linux API has no way
	// to differentiate between the two

	// create before a tap device with these commands:
	//
	// sudo ip tuntap add <devname> mode tap user <user>
	// sudo ip addr add 10.0.5.1/24 broadcast 10.0.5.255
	// sudo ip link set dev <devname> up
	//

	if (ioctl(fd, TUNSETIFF, (void *)&ifr) < 0) {
		err = errno;
		close(fd);
		errno = err;
		return -1;
	}

	// If we got back a different device than the one requested, e.g. because
	// the caller mistakenly passed in '%d' (yes, that's really in the Linux API)
	// then fail

	if (strncmp(ifr.ifr_name, dev, IFNAMSIZ) != 0) {
		close(fd);
		errno = ENODEV;
		return -1;
	}

	// Attempt a zero-sized write to the device. If the device was freshly created
	// (as opposed to attached to an existing ine) this will fail with EIO. Ignore
	// any other error return since that may indicate the device is up
	//
	// If this check produces a false positive then caller's later writes to fd will
	// fali with EIO, which is not great but at least we tried

	char buf[1] = { 0 };
	if (write(fd, buf, 0) == -1 && errno == EIO) {
		close(fd);
		errno = ENODEV;
		return -1;
	}

	return fd;
}

//---------------------------------- GET MAC ----------------------------------------------//
char* uhyve_get_mac(void)
{
	return netinfo.mac_str;
}

//---------------------------------- SET MAC ----------------------------------------------//

int uhyve_set_mac(void)
{
	int mac_is_set = 0;
	uint8_t guest_mac[6];

	char* str = getenv("HERMIT_NETIF_MAC");
	if (str)
	{
		const char *macptr = str;
		const char *v_macptr = macptr;
		// checking str is a valid MAC address
		int i = 0;
		int s = 0;
		while(*v_macptr) {
			if(isxdigit(*v_macptr)) {
				i++;
			} else if (*v_macptr == ':') {
				if (i / 2 - 1 != s++)
					break;
			} else {
				s = -1;
			}
			v_macptr++;
		}
		if (i != 12 || s != 5) {
			warnx("Malformed mac address: %s\n", macptr);
		} else {
			snprintf(netinfo.mac_str, sizeof(netinfo.mac_str), "%s", macptr);
			mac_is_set = 1;
		}
	}

	if (!mac_is_set) {
		int rfd = open("/dev/urandom", O_RDONLY);
		if(rfd == -1)
			err(1, "Could not open /dev/urandom\n");
		int ret;
		ret = read(rfd, guest_mac, sizeof(guest_mac));
		// compare the number of bytes read with the size of guest_mac
		assert(ret == sizeof(guest_mac));
		close(rfd);

		guest_mac[0] &= 0xfe;	// creats a random MAC-address in the locally administered
		guest_mac[0] |= 0x02;	// address range which can be used without conflict with other public devices
		// save the MAC address in the netinfo
		snprintf(netinfo.mac_str, sizeof(netinfo.mac_str),
			 "%02x:%02x:%02x:%02x:%02x:%02x",
	                 guest_mac[0], guest_mac[1], guest_mac[2],
			 guest_mac[3], guest_mac[4], guest_mac[5]);
	}

	return 0;
}

//-------------------------------------- SETUP NETWORK ---------------------------------------------//
int uhyve_net_init(const char *netif)
{
	if (netif == NULL) {
		err(1, "ERROR: no netif defined\n");
		return -1;
	}

	// attaching netif
	netfd = attach_linux_tap(netif);
	if (netfd < 0) {
		err(1, "Could not attach interface: %s\n", netif);
		exit(1);
	}

	uhyve_set_mac();

	return netfd;
}
