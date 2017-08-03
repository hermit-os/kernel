/*
 * Copyright (c) 2017, Stefan Lankes, RWTH Aachen University
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

#include <hermit/stddef.h>
#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/processor.h>
#include <hermit/mailbox.h>
#include <hermit/logging.h>
#include <hermit/virtio_net.h>
#include <hermit/virtio_ring.h>
#include <hermit/virtio_pci.h>
#include <hermit/virtio_net.h>
#include <asm/page.h>
#include <asm/io.h>
#include <asm/irq.h>
#include <asm/pci.h>
#include <lwip/sys.h>
#include <lwip/stats.h>
#include <lwip/netif.h>
#include <lwip/tcpip.h>
#include <lwip/snmp.h>
#include <lwip/ethip6.h>
#include <netif/etharp.h>
#include <net/vioif.h>

#define VENDOR_ID 0x1AF4
#define VIOIF_BUFFER_SIZE 0x2048
#define MIN(a, b)	(a) < (b) ? (a) : (b)
#define QUEUE_LIMIT 256

/* NOTE: RX queue is 0, TX queue is 1 - Virtio Std. §5.1.2  */
#define TX_NUM	1
#define RX_NUM	0

static struct netif* mynetif = NULL;

static inline void vioif_enable_interrupts(virt_queue_t* vq)
{
	vq->vring.used->flags = 0;
}

static inline void vioif_disable_interrupts(virt_queue_t* vq)
{
	vq->vring.used->flags = 1;
}

/*
 * @return error code
 * - ERR_OK: packet transferred to hardware
 * - ERR_CONN: no link or link failure
 * - ERR_IF: could not transfer to link (hardware buffer full?)
 */
static err_t vioif_output(struct netif* netif, struct pbuf* p)
{
	vioif_t* vioif = netif->state;
	virt_queue_t* vq = &vioif->queues[TX_NUM];
	struct pbuf *q;
	uint32_t i;
	uint16_t buffer_index;

	if (BUILTIN_EXPECT(p->tot_len > 1792, 0)) {
		LOG_ERROR("vioif_output: packet is longer than 1792 bytes\n");
		return ERR_IF;
	}

	for(buffer_index=0; buffer_index<vq->vring.num; buffer_index++) {
		if (!vq->vring.desc[buffer_index].len) {
			LOG_DEBUG("vioif_output: buffer %u is free\n", buffer_index);
			break;
		}
	}
	LOG_DEBUG("vioif: found free buffer %d\n", buffer_index);

	if (BUILTIN_EXPECT(buffer_index >= vq->vring.num, 0)) {
		LOG_ERROR("vioif_output: too many packets at once\n");
		return ERR_IF;
	}

#if ETH_PAD_SIZE
	pbuf_header(p, -ETH_PAD_SIZE); /* drop the padding word */
#endif

	const size_t hdr_sz = sizeof(struct virtio_net_hdr);
	// NOTE: packet is fully checksummed => all flags are set to zero
	memset((void*) (vq->virt_buffer + buffer_index * VIOIF_BUFFER_SIZE), 0x00, hdr_sz);

	vq->vring.desc[buffer_index].addr = vq->phys_buffer + buffer_index * VIOIF_BUFFER_SIZE;
	vq->vring.desc[buffer_index].len = p->tot_len + hdr_sz;
	vq->vring.desc[buffer_index].flags = 0;
	// we send only one buffer because it is large enough for our packet
	vq->vring.desc[buffer_index].next = 0; //(buffer_index+1) % vq->vring.num;


	/*
	 * q traverses through linked list of pbuf's
	 * This list MUST consist of a single packet ONLY
	 */
	for (q = p, i = 0; q != 0; q = q->next) {
		memcpy((void*) (vq->virt_buffer + hdr_sz + buffer_index * VIOIF_BUFFER_SIZE + i), q->payload, q->len);
		i += q->len;
	}

	// Add it in the available ring
	uint16_t index = vq->vring.avail->idx % vq->vring.num;
	vq->vring.avail->ring[index] = buffer_index;

	// besure that everything is written
	mb();

	vq->vring.avail->idx++;

	// besure that everything is written
	mb();

	/*
	 * Notify the changes
	 * NOTE: RX queue is 0, TX queue is 1 - Virtio Std. §5.1.2
	 */
    outportw(vioif->iobase+VIRTIO_PCI_QUEUE_NOTIFY, TX_NUM);

#if ETH_PAD_SIZE
	pbuf_header(p, ETH_PAD_SIZE); /* reclaim the padding word */
#endif

	LINK_STATS_INC(link.xmit);

	return ERR_OK;
}

static void vioif_rx_inthandler(struct netif* netif)
{
	vioif_t* vioif = mynetif->state;
	virt_queue_t* vq = &vioif->queues[RX_NUM];

	while(vq->last_seen_used != vq->vring.used->idx)
	{
		const size_t hdr_sz = sizeof(struct virtio_net_hdr);
		struct vring_used_elem* used = &vq->vring.used->ring[vq->last_seen_used % vq->vring.num];
		struct virtio_net_hdr* hdr = (struct virtio_net_hdr*) (vq->virt_buffer + used->id * VIOIF_BUFFER_SIZE);

		LOG_DEBUG("vq->vring.used->idx %d, vq->vring.used->flags %d, vq->last_seen_used %d\n", vq->vring.used->idx, vq->vring.used->flags, vq->last_seen_used);
		LOG_DEBUG("used id %d, len %d\n", used->id, used->len);
		LOG_DEBUG("hdr len %d, flags %d\n", hdr->hdr_len, hdr->flags);

		struct pbuf* p = pbuf_alloc(PBUF_RAW, used->len, PBUF_POOL);
		if (p) {
			uint16_t pos;
			struct pbuf* q;

#if ETH_PAD_SIZE
			pbuf_header(p, -ETH_PAD_SIZE); /* drop the padding word */
#endif
			for(q=p, pos=0; q!=NULL; q=q->next) {
				memcpy((uint8_t*) q->payload,
					(uint8_t*) (vq->virt_buffer + hdr_sz + used->id * VIOIF_BUFFER_SIZE + pos),
					q->len);
				pos += q->len;
			}
#if ETH_PAD_SIZE
			pbuf_header(p, ETH_PAD_SIZE); /* reclaim the padding word */
#endif
			LINK_STATS_INC(link.recv);

			// forward packet to LwIP
			netif->input(p, netif);
		} else {
			LOG_ERROR("vioif_rx_inthandler: not enough memory!\n");
			LINK_STATS_INC(link.memerr);
			LINK_STATS_INC(link.drop);
			goto oom;
		}

		vq->vring.avail->ring[vq->vring.avail->idx % vq->vring.num] = used->id;
		vq->vring.avail->idx++;
		vq->last_seen_used++;
	}

oom:
	vioif->polling = 0;
	vioif_enable_interrupts(vq);
	mb();
}


/* this function is called in the context of the tcpip thread or the irq handler (by using NO_SYS) */
static void vioif_poll(void* ctx)
{
	vioif_rx_inthandler(mynetif);
}

static void vioif_handler(struct state* s)
{
	vioif_t* vioif = mynetif->state;

	LOG_DEBUG("vioif: receive interrupt\n");

	// reset interrupt by reading the isr port
	uint8_t isr = inportb(vioif->iobase+VIRTIO_PCI_ISR);

	// do we receiven an interrupt for this device?
	if (!(isr & 0x01))
		return;

	// free TX queue
	virt_queue_t* vq = &vioif->queues[1];

	vioif_disable_interrupts(vq);
	while(vq->last_seen_used != vq->vring.used->idx)
	{
		struct vring_used_elem* used = &vq->vring.used->ring[vq->last_seen_used % vq->vring.num];
		LOG_DEBUG("consumed TX elements: index %u, len %u\n", used->id, used->len);
		// mark as free
		vq->vring.desc[used->id].len = 0;
		vq->last_seen_used++;
	}
	vioif_enable_interrupts(vq);
	mb();

	// check RX qeueue
	vq = &vioif->queues[0];
	vioif_disable_interrupts(vq);
	if (!vioif->polling && (vq->last_seen_used != vq->vring.used->idx))
	{
#if NO_SYS
		vioif_poll(NULL);
#else
		if (tcpip_callback_with_block(vioif_poll, NULL, 0) == ERR_OK) {
			vioif->polling = 1;
		} else {
			LOG_ERROR("rtl8139if_handler: unable to send a poll request to the tcpip thread\n");
		}
#endif
	} else vioif_enable_interrupts(vq);
	mb();
}

static int vioif_queue_setup(vioif_t* dev)
{
	virt_queue_t* vq;
	uint32_t total_size;
	unsigned int num;

	for (uint32_t index=0; index<VIOIF_NUM_QUEUES; index++) {
		vq = &dev->queues[index];

	    memset(vq, 0x00, sizeof(virt_queue_t));

		// determine queue size
		outportw(dev->iobase+VIRTIO_PCI_QUEUE_SEL, index);
		num = inportw(dev->iobase+VIRTIO_PCI_QUEUE_NUM);
		if (!num) return -1;

		LOG_INFO("vioif: queue_size %u (index %u)\n", num, index);

		total_size = vring_size(num, PAGE_SIZE);

		// allocate and init memory for the virtual queue
		void* vring_base = page_alloc(total_size, VMA_READ|VMA_WRITE|VMA_CACHEABLE);
		if (BUILTIN_EXPECT(!vring_base, 0)) {
			LOG_INFO("Not enough memory to create queue %u\n", index);
			return -1;
		}
		memset((void*)vring_base, 0x00, total_size);
		vring_init(&vq->vring, num, vring_base, PAGE_SIZE);

		if (num > QUEUE_LIMIT) {
			vq->vring.num = num = QUEUE_LIMIT;
			LOG_INFO("vioif: set queue limit to %u (index %u)\n", vq->vring.num, index);
		}

		vq->virt_buffer = (uint64_t) page_alloc(num*VIOIF_BUFFER_SIZE, VMA_READ|VMA_WRITE|VMA_CACHEABLE);
		if (BUILTIN_EXPECT(!vq->virt_buffer, 0)) {
			LOG_INFO("Not enough memory to create buffer %u\n", index);
			return -1;
		}
		vq->phys_buffer = virt_to_phys(vq->virt_buffer);

		for(int i=0; i<num; i++) {
			vq->vring.desc[i].addr = vq->phys_buffer + i * VIOIF_BUFFER_SIZE;
			if (index == RX_NUM) {
				/* NOTE: RX queue is 0, TX queue is 1 - Virtio Std. §5.1.2  */
				vq->vring.desc[i].len = VIOIF_BUFFER_SIZE;
				vq->vring.desc[i].flags = VRING_DESC_F_WRITE;
				vq->vring.avail->ring[vq->vring.avail->idx % num] = i;
				vq->vring.avail->idx++;
			}
		}

		// register buffer
		outportw(dev->iobase+VIRTIO_PCI_QUEUE_SEL, index);
		outportl(dev->iobase+VIRTIO_PCI_QUEUE_PFN, virt_to_phys((size_t) vring_base) >> PAGE_BITS);
	}

	return 0;
}

err_t vioif_init(struct netif* netif)
{
	static uint8_t num = 0;
	vioif_t* vioif;
	pci_info_t pci_info;
	int i;

	LWIP_ASSERT("netif != NULL", (netif != NULL));

	for(i=0x100; i<=0x103F; i++) {
		if ((pci_get_device_info(VENDOR_ID, i, 1, &pci_info, 1) == 0)) {
			LOG_INFO("Found vioif (Vendor ID 0x%x, Device Id 0x%x)\n", VENDOR_ID, i);
			break;
		}
	}

	if (i > 0x103F)
		return ERR_ARG;

	vioif = kmalloc(sizeof(vioif_t));
	if (!vioif) {
		LOG_ERROR("virtioif_init: out of memory\n");
		return ERR_MEM;
	}
	memset(vioif, 0x00, sizeof(vioif_t));

	vioif->iomem = pci_info.base[1];
	vioif->iobase = pci_info.base[0];
	vioif->irq = pci_info.irq;
	LOG_INFO("vioif uses IRQ %d and IO port 0x%x, IO men 0x%x\n", (int32_t) vioif->irq, vioif->iobase, vioif->iomem);

	// reset interface
	outportb(vioif->iobase + VIRTIO_PCI_STATUS, 0);
	LOG_INFO("vioif status: 0x%x\n", (uint32_t) inportb(vioif->iobase + VIRTIO_PCI_STATUS));

	// tell the device that we have noticed it
	outportb(vioif->iobase + VIRTIO_PCI_STATUS, VIRTIO_CONFIG_S_ACKNOWLEDGE);
	// tell the device that we will support it.
	outportb(vioif->iobase + VIRTIO_PCI_STATUS, VIRTIO_CONFIG_S_ACKNOWLEDGE|VIRTIO_CONFIG_S_DRIVER);

	LOG_INFO("host features 0x%x\n", inportl(vioif->iobase + VIRTIO_PCI_HOST_FEATURES));

	uint32_t features = inportl(vioif->iobase + VIRTIO_PCI_HOST_FEATURES);
	uint32_t required = (1UL << VIRTIO_NET_F_MAC) | (1UL << VIRTIO_NET_F_STATUS);

	if ((features & required) != required) {
		LOG_ERROR("Host isn't able to fulfill HermireCore's requirements\n");
		outportb(vioif->iobase + VIRTIO_PCI_STATUS, VIRTIO_CONFIG_S_FAILED);
		kfree(vioif);
		return ERR_ARG;
	}

	required = features;
	required &= ~(1UL << VIRTIO_NET_F_CTRL_VQ);
    required &= ~(1UL << VIRTIO_NET_F_GUEST_TSO4);
    required &= ~(1UL << VIRTIO_NET_F_GUEST_TSO6);
    required &= ~(1UL << VIRTIO_NET_F_GUEST_UFO);
    required &= ~(1UL << VIRTIO_RING_F_EVENT_IDX);
    required &= ~(1UL << VIRTIO_NET_F_MRG_RXBUF);
	required &= ~(1UL << VIRTIO_NET_F_MQ);

	LOG_INFO("wanted guest features 0x%x\n", required);
	outportl(vioif->iobase + VIRTIO_PCI_GUEST_FEATURES, required);
	vioif->features = inportl(vioif->iobase + VIRTIO_PCI_GUEST_FEATURES);
	LOG_INFO("current guest features 0x%x\n", vioif->features);

	// tell the device that the features are OK
	outportb(vioif->iobase + VIRTIO_PCI_STATUS, VIRTIO_CONFIG_S_ACKNOWLEDGE|VIRTIO_CONFIG_S_DRIVER|VIRTIO_CONFIG_S_FEATURES_OK);

	// check if the host accept these features
	uint8_t status = inportb(vioif->iobase + VIRTIO_PCI_STATUS);
	if (!(status & VIRTIO_CONFIG_S_FEATURES_OK)) {
		LOG_ERROR("device features are ignored: status 0x%x\n", (uint32_t) status);
		outportb(vioif->iobase + VIRTIO_PCI_STATUS, VIRTIO_CONFIG_S_FAILED);
		kfree(vioif);
		return ERR_ARG;
	}

	/* hardware address length */
	netif->hwaddr_len = ETHARP_HWADDR_LEN;

	// determine the mac address of this card
	LWIP_DEBUGF(NETIF_DEBUG, ("vioif_init: MAC address "));
	for (uint8_t tmp8=0; tmp8<ETHARP_HWADDR_LEN; tmp8++) {
		netif->hwaddr[tmp8] = inportb(vioif->iobase + VIRTIO_PCI_CONFIG_OFF(vioif->msix_enabled) + tmp8);
		LWIP_DEBUGF(NETIF_DEBUG, ("%02x ", netif->hwaddr[tmp8]));
	}
	LWIP_DEBUGF(NETIF_DEBUG, ("\n"));

	// Setup virt queues
	if (BUILTIN_EXPECT(vioif_queue_setup(vioif) < 0, 0)) {
		outportb(vioif->iobase + VIRTIO_PCI_STATUS, VIRTIO_CONFIG_S_FAILED);
		kfree(vioif);
		return ERR_ARG;
	}

	netif->state = vioif;
	mynetif = netif;

	irq_install_handler(vioif->irq+32, vioif_handler);

	/*
	 * Initialize the snmp variables and counters inside the struct netif.
	 * The last argument should be replaced with your link speed, in units
	 * of bits per second.
	 */
	NETIF_INIT_SNMP(netif, snmp_ifType_ethernet_csmacd, 1000);

	/* administrative details */
	netif->name[0] = 'e';
	netif->name[1] = 'n';
	netif->num = num;
	num++;
	/* downward functions */
	netif->output = etharp_output;
	netif->linkoutput = vioif_output;
	/* set maximum transfer unit
	 * Google Compute Platform supports only a MTU of 1460
	 */
	netif->mtu = 1460;
	/* broadcast capability */
	netif->flags |= NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_IGMP | NETIF_FLAG_LINK_UP | NETIF_FLAG_MLD6;
#if LWIP_IPV6
	netif->output_ip6 = ethip6_output;
	netif_create_ip6_linklocal_address(netif, 1);
	netif->ip6_autoconfig_enabled = 1;
#endif

	// tell the device that the drivers is initialized
	outportb(vioif->iobase + VIRTIO_PCI_STATUS, VIRTIO_CONFIG_S_ACKNOWLEDGE|VIRTIO_CONFIG_S_DRIVER|VIRTIO_CONFIG_S_DRIVER_OK|VIRTIO_CONFIG_S_FEATURES_OK);

	LOG_INFO("vioif status: 0x%x\n", (uint32_t) inportb(vioif->iobase + VIRTIO_PCI_STATUS));
	LOG_INFO("vioif link is %s\n",
		inportl(vioif->iobase + VIRTIO_PCI_CONFIG_OFF(vioif->msix_enabled) + ETHARP_HWADDR_LEN) & VIRTIO_NET_S_LINK_UP ? "up" : "down");

	return ERR_OK;
}
