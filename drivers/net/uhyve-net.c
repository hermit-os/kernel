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


#include <hermit/stddef.h>
#include <hermit/stdio.h>
#include <hermit/tasks.h>
#include <hermit/errno.h>
#include <hermit/syscall.h>
#include <hermit/spinlock.h>
#include <hermit/semaphore.h>
#include <hermit/time.h>
#include <hermit/rcce.h>
#include <hermit/memory.h>
#include <hermit/signal.h>
#include <hermit/mailbox.h>
#include <hermit/logging.h>
#include <asm/io.h>
#include <asm/irq.h>
#include <sys/poll.h>
#include <lwip/sys.h>
#include <lwip/netif.h>
#include <lwip/tcpip.h>
#include <lwip/snmp.h>
#include <lwip/sockets.h>
#include <lwip/err.h>
#include <lwip/stats.h>
#include <lwip/ethip6.h>
#include <netif/etharp.h>

#include "uhyve-net.h"

#define UHYVE_IRQ	11

static int8_t uhyve_net_init_ok = 0;
static struct netif* mynetif = NULL;

static int uhyve_net_write_sync(uint8_t *data, int n)
{
	volatile uhyve_netwrite_t uhyve_netwrite;
	uhyve_netwrite.data = (uint8_t*)virt_to_phys((size_t)data);
	uhyve_netwrite.len = n;
	uhyve_netwrite.ret = 0;

	outportl(UHYVE_PORT_NETWRITE, (unsigned)virt_to_phys((size_t)&uhyve_netwrite));

	return uhyve_netwrite.ret;
}

int uhyve_net_stat(void)
{
        volatile uhyve_netstat_t uhyve_netstat;

        outportl(UHYVE_PORT_NETSTAT, (unsigned)virt_to_phys((size_t)&uhyve_netstat));

        return uhyve_netstat.status;
}

static int uhyve_net_read_sync(uint8_t *data, int *n)
{
	volatile uhyve_netread_t uhyve_netread;

	uhyve_netread.data = (uint8_t*)virt_to_phys((size_t)data);
	uhyve_netread.len = *n;
	uhyve_netread.ret = 0;

	outportl(UHYVE_PORT_NETREAD, (unsigned)virt_to_phys((size_t)&uhyve_netread));
	*n = uhyve_netread.len;

	return uhyve_netread.ret;
}

static char mac_str[18];
static char *hermit_net_mac_str(void)
{
	volatile uhyve_netinfo_t uhyve_netinfo;

	outportl(UHYVE_PORT_NETINFO, (unsigned)virt_to_phys((size_t)&uhyve_netinfo));
	memcpy(mac_str, (void *)&uhyve_netinfo.mac_str, 18);

	return mac_str;
}

static inline uint8_t dehex(char c)
{
        if (c >= '0' && c <= '9')
                return (c - '0');
        else if (c >= 'a' && c <= 'f')
                return 10 + (c - 'a');
        else if (c >= 'A' && c <= 'F')
                return 10 + (c - 'A');
        else
                return 0;
}

//---------------------------- OUTPUT --------------------------------------------

static err_t uhyve_netif_output(struct netif* netif, struct pbuf* p)
{
	uhyve_netif_t* uhyve_netif = netif->state;
	uint8_t transmitid = uhyve_netif->tx_queue % TX_BUF_NUM;
	uint32_t i;
	struct pbuf *q;

	if(BUILTIN_EXPECT((uhyve_netif->tx_queue - uhyve_netif->tx_complete) > (TX_BUF_NUM - 1), 0)) {
		LOG_ERROR("uhyve_netif_output: too many packets at once\n");
		return ERR_IF;
	}

	if(BUILTIN_EXPECT(p->tot_len > 1792, 0)) {
		LOG_ERROR("uhyve_netif_output: packet (%i bytes) is longer than 1792 bytes\n", p->tot_len);
		return ERR_IF;
	}

	if(uhyve_netif->tx_inuse[transmitid] == 1) {
		LOG_ERROR("uhyve_netif_output: %i already inuse\n", transmitid);
		return ERR_IF;
	}

	uhyve_netif->tx_queue++;
	uhyve_netif->tx_inuse[transmitid] = 1;

#if ETH_PAD_SIZE
	pbuf_header(p, -ETH_PAD_SIZE); /*drop padding word */
#endif

	/*
	 * q traverses through linked list of pbuf's
	 * This list MUST consist of a single packet ONLY
	 */
	for (q = p, i = 0; q != 0; q = q->next) {
		memcpy(uhyve_netif->tx_buf[transmitid] + i, q->payload, q->len);
		i += q->len;
	}
	// send the packet
	uhyve_net_write_sync(uhyve_netif->tx_buf[transmitid], p->tot_len);

#if ETH_PAD_SIZE
	pbuf_header(p, ETH_PAD_SIZE); /* reclaim the padding word */
#endif

	LINK_STATS_INC(link.xmit);

	uhyve_netif->tx_complete++;
	uhyve_netif->tx_inuse[transmitid] = 0;
//	LOG_INFO("Transmit OK | queue = %i, complete = %i \n", uhyve_netif->tx_queue, uhyve_netif->tx_complete);

	return ERR_OK;
}

static void consume_packet(void* ctx)
{
	struct pbuf *p = (struct pbuf*) ctx;

	mynetif->input(p, mynetif);
}

//------------------------------- POLLING ----------------------------------------

static void uhyve_netif_poll(void)
{
	if (!uhyve_net_init_ok)
		return;

	uhyve_netif_t* uhyve_netif = mynetif->state;
	int len = RX_BUF_LEN;
	struct pbuf *p = NULL;
	struct pbuf *q;

	if (uhyve_net_read_sync(uhyve_netif->rx_buf, &len) == 0)
	{
#if ETH_PAD_SIZE
		len += ETH_PAD_SIZE; /*allow room for Ethernet padding */
#endif
		p = pbuf_alloc(PBUF_RAW, len, PBUF_POOL);
		if(p) {
#if ETH_PAD_SIZE
			pbuf_header(p, -ETH_PAD_SIZE); /*drop the padding word */
#endif
			uint8_t pos = 0;
			for (q=p; q!=NULL; q=q->next) {
				memcpy((uint8_t*) q->payload, uhyve_netif->rx_buf + pos, q->len);
				pos += q->len;
			}
#if ETH_PAD_SIZE
			pbuf_header(p, ETH_PAD_SIZE); /*reclaim the padding word */
#endif


			//forward packet to the IP thread
			if (tcpip_callback_with_block(consume_packet, p, 0) == ERR_OK) {
				LINK_STATS_INC(link.recv);
			} else {
				LINK_STATS_INC(link.drop);
				pbuf_free(p);
			}
		} else {
			LOG_ERROR("uhyve_netif_poll: not enough memory!\n");
			LINK_STATS_INC(link.memerr);
			LINK_STATS_INC(link.drop);
		}
	}
}

static void uhyve_irqhandler(struct state* s)
{
	uhyve_netif_poll();
}

//--------------------------------- INIT -----------------------------------------

err_t uhyve_netif_init (struct netif* netif)
{
	uhyve_netif_t* uhyve_netif;
	uint8_t tmp8 = 0;
	static uint8_t num = 0;

	uhyve_netif = kmalloc(sizeof(uhyve_netif_t));
	if (!uhyve_netif) {
		LOG_ERROR("uhyve_netif_init: out of memory\n");
		return ERR_MEM;
	}

	memset(uhyve_netif, 0x00, sizeof(uhyve_netif_t));

	uhyve_netif->rx_buf = page_alloc(RX_BUF_LEN + 16 /* header size */, VMA_READ|VMA_WRITE);
	if (!(uhyve_netif->rx_buf)) {
		LOG_ERROR("uhyve_netif_init: out of memory\n");
		kfree(uhyve_netif);
		return ERR_MEM;
	}
	memset(uhyve_netif->rx_buf, 0x00, RX_BUF_LEN + 16);

	uhyve_netif->tx_buf[0] = page_alloc(TX_BUF_NUM * TX_BUF_LEN, VMA_READ|VMA_WRITE);
	if (!(uhyve_netif->tx_buf[0])) {
		LOG_ERROR("uhyve_netif_init: out of memory\n");
		page_free(uhyve_netif->rx_buf, RX_BUF_LEN + 16);
		kfree(uhyve_netif);
		return ERR_MEM;
	}
	memset(uhyve_netif->tx_buf[0], 0x00, TX_BUF_NUM * TX_BUF_LEN);
	for (int i = 1; i < TX_BUF_NUM; i++) {
		uhyve_netif->tx_buf[i] = uhyve_netif->tx_buf[0] + i*TX_BUF_LEN;
	}

	netif->state = uhyve_netif;
	mynetif = netif;

	netif->hwaddr_len = ETHARP_HWADDR_LEN;

	LOG_INFO("uhyve_netif_init: Found uhyve_net interface\n");

	LWIP_DEBUGF(NETIF_DEBUG, ("uhyve_netif_init: MAC address "));
	char *hermit_mac = hermit_net_mac_str();
	for (tmp8=0; tmp8 < ETHARP_HWADDR_LEN; tmp8++) {
		netif->hwaddr[tmp8] = dehex(*hermit_mac++) << 4;
		netif->hwaddr[tmp8] |= dehex(*hermit_mac++);
		hermit_mac++;
		LWIP_DEBUGF(NETIF_DEBUG, ("%02x ", netif->hwaddr[tmp8]));
	}
	LWIP_DEBUGF(NETIF_DEBUG, ("\n"));
	uhyve_netif->ethaddr = (struct eth_addr *)netif->hwaddr;

	LOG_INFO("uhye_netif uses irq %d\n", UHYVE_IRQ);
	irq_install_handler(32+UHYVE_IRQ, uhyve_irqhandler);

	/*
	 * Initialize the snmp variables and counters inside the struct netif.
	 * The last argument should be replaced with your link speed, in units
	 * of bits per second.
	 */
	NETIF_INIT_SNMP(netif, snmp_ifType_ethernet_csmacd, 1000);

	netif->name[0] = 'e';
	netif->name[1] = 'n';
	netif->num = num++;
	/* downward functions */
	netif->output = etharp_output;
	netif->linkoutput = uhyve_netif_output;
	/* maximum transfer unit */
	netif->mtu = 1500;
	/* broadcast capability */
	netif->flags |= NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_IGMP | NETIF_FLAG_LINK_UP | NETIF_FLAG_MLD6;

#if LWIP_IPV6
	netif->output_ip6 = ethip6_output;
	netif_create_ip6_linklocal_address(netif, 1);
	netif->ip6_autoconfig_enabled = 1;
#endif

	LOG_INFO("uhyve_netif_init: OK\n");
	uhyve_net_init_ok = 1;

	return ERR_OK;
}
