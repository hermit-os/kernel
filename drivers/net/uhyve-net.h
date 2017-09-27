/*
 * Copyright 2017 RWTH Aachen University
 * Author(s): Tim van de Kamp <tim.van.de.kamp@rwth-aachen.de>
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
 *
 * This code based mostly on the online manual http://www.lowlevel.eu/wiki/RTL8139
 */

#ifndef __NET_UHYVE_NET_H__
#define __NET_UHYVE_NET_H__

#include <hermit/stddef.h>
#include <hermit/spinlock.h>

#define MIN(a, b)	(a) < (b) ? (a) : (b)

#define RX_BUF_LEN 2048
#define TX_BUF_LEN 2048
#define TX_BUF_NUM 1		//number of tx buffer

#define UHYVE_PORT_NETINFO      0x505
#define UHYVE_PORT_NETWRITE     0x506
#define UHYVE_PORT_NETREAD      0x507
#define UHYVE_PORT_NETSTAT	0x508

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

/*
 * Helper struct to hold private data used to operate your ethernet interface.
 */
// NETIF state struct
typedef struct uhyve_netif {
	struct eth_addr *ethaddr;
	/* Add whatever per-interface state that is needed here. */
	uint8_t* tx_buf[TX_BUF_NUM];
	uint32_t tx_queue;
	uint32_t tx_complete;
	uint8_t tx_inuse[TX_BUF_NUM];
	uint8_t* rx_buf;
} uhyve_netif_t;

err_t uhyve_netif_init(struct netif* netif);
int uhyve_net_stat(void);

#endif
