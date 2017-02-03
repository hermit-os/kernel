/*
 * Copyright 2011 Carl-Benedikt Krueger, Chair for Operating Systems,
 *                                       RWTH Aachen University
 * Copyright 2015 Stefan Lankes, RWTH Aachen University
 * All rights reserved.
 *
 * This software is available to you under a choice of one of two
 * licenses.  You may choose to be licensed under the terms of the GNU
 * General Public License (GPL) Version 2 (https://www.gnu.org/licenses/gpl-2.0.txt)
 * or the BSD license below:
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
 * mmnif.c	---	memmory mapped interface
 *
 * Virtual IP Interface initially designed for the concept processor SCC
 * and now adapted for HermitCore
 */

/*
 * 15th October 2011:
 * - Redesign of the interrupt handling (by Stefan Lankes)
 * - Add iRCCE support (by Stefan Lankes)
 * - Extending the BSD socket interface
 */

/*
 * 14th September 2015:
 * - Adapted for HermitCore (by Stefan Lankes)
 * - Removing of SCC related code (by Stefan Lankes)
 */

#include <hermit/stddef.h>
#include <hermit/logging.h>

#include <lwip/netif.h>		/* lwip netif */
#include <lwip/netifapi.h>
#include <lwip/stats.h>		/* inteface stats */
#include <netif/etharp.h>	/* ethernet arp packets */
#include <lwip/ip.h>		/* struct iphdr */
#include <lwip/tcpip.h>		/* tcpip_input() */
#include <lwip/sockets.h>
#include <lwip/ip_addr.h>

#include <hermit/mailbox.h>	/* mailbox_ptr_t */
#include <hermit/semaphore.h>
#include <hermit/spinlock.h>
#include <hermit/time.h>
#include <hermit/islelock.h>
#include <asm/page.h>
#include <asm/irq.h>
#include <asm/irqflags.h>
#include <asm/apic.h>

#include <net/mmnif.h>

#define TRUE	1
#define FALSE	0

#define DEBUGPRINTF(x,...)  LWIP_DEBUGF(NETIF_DEBUG, (x, ##__VA_ARGS__))

#define DEBUG_MMNIF
//#define DEBUG_MMNIF_PACKET

#define MMNIF_AUTO_SOCKET_TIMEOUT       500

#define MMNIF_RX_BUFFERLEN		(28*1024)
#define MMNIF_IRQ			122

#ifdef DEBUG_MMNIF
#include <net/util.h>		/* hex dump */
#endif

/*  define constants
 *  regarding the driver & its configuration
 */
#define MMNIF_MAX_DESCRIPTORS		64

#define MMNIF_STATUS_FREE		0x00
#define MMNIF_STATUS_PENDING            0x01
#define MMNIF_STATUS_RDY		0x02
#define MMNIF_STATUS_INPROC		0x03
#define MMNIF_STATUS_PROC		0x04

// id of the HermitCore isle
extern int32_t isle;
extern int32_t possible_isles;
extern char* phy_isle_locks;

static spinlock_irqsave_t locallock;

/* "message passing buffer" specific constants:
 * - start address
 * - size
 */
extern char* header_start_address;
extern char* header_phy_start_address;
extern unsigned int header_size;
extern char* heap_start_address;
extern char* heap_phy_start_address;
extern unsigned int heap_size;

/*
 * the memory mapped network device
 */
static struct netif* mmnif_dev = NULL;

/*
 */
typedef struct mmnif_device_stats {
	/* device stats (granularity in packets):
	 * - recieve errors
	 * - recieve successes
	 * - recieved bytes
	 * - transmit errors
	 * - transmit successes
	 * - transmitted bytes
	 */
	unsigned int rx_err;
	unsigned int rx;
	unsigned int rx_bytes;
	unsigned int tx_err;
	unsigned int tx;
	unsigned int tx_bytes;

	/* Heuristics :
	 * - how many times an budget overflow occured
	 * - how many times the polling thread polled without recieving a new message
	 */
	unsigned int bdg_overflow;
	unsigned int pll_empty;
} mmnif_device_stats_t;

/* receive descror structure */
typedef struct rx_desc {
	/* stat : status of the descriptor
	 * len  : length of the packet
	 * addr : memory address of the packet
	 * id   : packet id
	 */
	uint8_t stat;
	uint16_t len;
	size_t addr;
} rx_desc_t;

/* receive ring buffer structure */
typedef struct mm_rx_buffer {
	/* memory "pseudo-ring/heap"
	 * packets are always in one single chunk of memory
	 * head : head of allocated memory region
	 * tail : tail of allocated memory region
	 */
	uint16_t head;
	uint16_t tail;

	/* descritpor queue
	 * desc_table : descriptor table
	 * dcount : descriptor's free in queue
	 * dread : next descriptor to read
	 * dwrite : next descriptor to write
	 */
	rx_desc_t desc_table[MMNIF_MAX_DESCRIPTORS];
	uint8_t dcount;
	uint8_t dread;
	uint8_t dwrite;
} mm_rx_buffer_t;

typedef struct mmnif {
	struct mmnif_device_stats stats;

	/* Interface constants:
	 * - ehternet address
	 * - local ip address
	 */
	struct eth_addr *ethaddr;
	uint32_t ipaddr;

	// checks the TCPIP thread already the rx buffers?
	volatile uint8_t check_in_progress;

	/* memory interaction variables:
	 * - pointer to recive buffer
	 */
	volatile mm_rx_buffer_t *rx_buff;
	uint8_t *rx_heap;

	/* semaphore to regulate polling vs. interrupts
	 */
	sem_t com_poll;
} mmnif_t;

// alread initialized by Linux
static islelock_t* isle_locks = NULL;

// forward declaration
static void mmnif_irqhandler(struct state* s);

/*
 *	memory maped interface helper functions
 */

/* trigger an interrupt on the remote processor
 * so he knows there is a packet to read
 */
inline static int mmnif_trigger_irq(int dest_ip)
{
	int dest;

	if (dest_ip == 1)
		dest = 0;
	else
		dest = 0;	// TODO: determine physical apic id of the destination

	return apic_send_ipi(dest, MMNIF_IRQ);
}

/* mmnif_print_stats(): Print the devices stats of the
 * current device
 */
static void mmnif_print_stats(void)
{
	mmnif_t *mmnif;

	if (!mmnif_dev)
	{
		LOG_INFO("mmnif_print_stats(): the device is not initialized yet.\n");
		return;
	}

	mmnif = (mmnif_t *) mmnif_dev->state;
	LOG_INFO("/dev/mmnif - stats:\n");
	LOG_INFO("Received: %d packets successfull\n", mmnif->stats.rx);
	LOG_INFO("Received: %d bytes\n", mmnif->stats.rx_bytes);
	LOG_INFO("Received: %d packets containuing errors\n", mmnif->stats.rx_err);
	LOG_INFO("Transmitted: %d packests successfull\n", mmnif->stats.tx);
	LOG_INFO("Transmitted: %d bytes\n", mmnif->stats.tx_bytes);
	LOG_INFO("Transmitted: %d packests were dropped due to errors\n", mmnif->stats.tx_err);
}

/* mmnif_print_driver_status
 *
 */
void mmnif_print_driver_status(void)
{
	mmnif_t *mmnif;
	int i;

	if (!mmnif_dev)
	{
		LOG_ERROR("mmnif_print_driver_status(): the device is not initialized yet.\n");
		return;
	}

	mmnif = (mmnif_t *) mmnif_dev->state;
	LOG_INFO("/dev/mmnif driver status: \n\n");
	LOG_INFO("rx_buf: 0xp\n", mmnif->rx_buff);
	LOG_INFO("free descriptors : %d\n\n", mmnif->rx_buff->dcount);
	LOG_INFO("descriptor table: (only print descriptors in use)\n");
	LOG_INFO("status\taddr\tsize\n");

	for (i = 0; i < MMNIF_MAX_DESCRIPTORS; i++)
	{
		if (mmnif->rx_buff->desc_table[i].stat != 0)
			LOG_INFO("0x%.2X\t%p\t%X\t\n",
				    mmnif->rx_buff->desc_table[i].stat,
				    mmnif->rx_buff->desc_table[i].addr,
				    mmnif->rx_buff->desc_table[i].len);
	}

	LOG_INFO("ring heap start addr: %p\n", mmnif->rx_buff + sizeof(mm_rx_buffer_t));
	LOG_INFO("head: 0x%X\ttail: 0x%X\n", mmnif->rx_buff->head, mmnif->rx_buff->tail);
	mmnif_print_stats();
}

/*
 *	memory maped interface main functions
 */

/* mmnif_get_destination(): low level transmid helper function
 * this function deals with some HW details, it checks to wich core this packet
 * should be routed and returns the destination
 */
static uint8_t mmnif_get_destination(struct netif *netif, struct pbuf *p)
{
	struct ip_hdr *iphdr;
	ip4_addr_p_t ip;

	/* grab the destination ip address out of the ip header
	 * for internal routing the last ocet is interpreted as core ID.
	 */
	iphdr = (struct ip_hdr *)(p->payload);
	ip = iphdr->dest;

	// forward packet to Linux (= isle 1), if the destination IP is a real address
	if ((ip4_addr1(&ip) != 192) || (ip4_addr2(&ip) != 168) || (ip4_addr3(&ip) != 28))
		return 1;

	return ip4_addr4(&ip);
}

/* mmnif_rxbuff_alloc():
 * this function allocates a continues chunk of memory
 * right inside of the buffer which is used for communication
 * with the remote end
 */
static size_t mmnif_rxbuff_alloc(uint8_t dest, uint16_t len)
{
	size_t ret = 0;
	volatile mm_rx_buffer_t *rb = (mm_rx_buffer_t *) ((char *)header_start_address + (dest - 1) * header_size);
	char *memblock = (char *)heap_start_address + (dest - 1) * heap_size;

//        if (rb->tail > rb->head)
//             if ((MMNIF_RX_BUFFERLEN - rb->tail < len)&&(rb->head < len))
//                 return NULL;
//        else
//            if ((rb->head - rb->tail < len)&&(rb->tail != rb->head))
//                return NULL;

	islelock_lock(isle_locks + (dest-1));
	if (rb->dcount)
	{
		if (rb->tail > rb->head)
		{
			if (MMNIF_RX_BUFFERLEN - rb->tail > len)
			{
				rb->desc_table[rb->dwrite].stat = MMNIF_STATUS_PENDING;
				ret = (size_t) (memblock + rb->tail);
				rb->desc_table[rb->dwrite].addr = ret;
				rb->desc_table[rb->dwrite].len = len;
				rb->dcount--;
				rb->dwrite = (rb->dwrite + 1) % MMNIF_MAX_DESCRIPTORS;
				rb->tail = (rb->tail + len);
			} else if (rb->head > len) {
				rb->desc_table[rb->dwrite].stat = MMNIF_STATUS_PENDING;
				ret = (size_t) memblock;
				rb->desc_table[rb->dwrite].addr = ret;
				rb->desc_table[rb->dwrite].len = len;
				rb->dcount--;
				rb->dwrite = (rb->dwrite + 1) % MMNIF_MAX_DESCRIPTORS;
				rb->tail = len;
			}
		} else {
			if (rb->head - rb->tail > len)
			{
				rb->desc_table[rb->dwrite].stat = MMNIF_STATUS_PENDING;
				ret = (size_t) (memblock + rb->tail);
				rb->desc_table[rb->dwrite].addr = ret;
				rb->desc_table[rb->dwrite].len = len;
				rb->dcount--;
				rb->dwrite = (rb->dwrite + 1) % MMNIF_MAX_DESCRIPTORS;
				rb->tail = (rb->tail + len);
			} else if (rb->tail == rb->head) {
				if (MMNIF_RX_BUFFERLEN - rb->tail < len)
				{
					rb->tail = 0;
					if (rb->dread == rb->dwrite)
						rb->head = 0;
				}
				rb->desc_table[rb->dwrite].stat = MMNIF_STATUS_PENDING;
				ret = (size_t) (memblock + rb->tail);
				rb->desc_table[rb->dwrite].addr = ret;
				rb->desc_table[rb->dwrite].len = len;
				rb->dcount--;
				rb->dwrite = (rb->dwrite + 1) % MMNIF_MAX_DESCRIPTORS;
				rb->tail = (rb->tail + len);
			}
		}
	}
	islelock_unlock(isle_locks + (dest-1));

	return ret;
}

/* mmnif_commit_packet: this function set the state of the (in advance)
 * allocated packet to RDY so the recieve queue knows that it can be
 * processed further
 */
static int mmnif_commit_packet(uint8_t dest, uint32_t addr)
{
	volatile mm_rx_buffer_t *rb = (mm_rx_buffer_t *) ((char *)header_start_address + (dest - 1) * header_size);
	uint32_t i;

	for (i = 0; i < MMNIF_MAX_DESCRIPTORS; i++)
	{
		if (rb->desc_table[i].addr == addr
		    && rb->desc_table[i].stat == MMNIF_STATUS_PENDING)
		{
			rb->desc_table[i].stat = MMNIF_STATUS_RDY;

			return 0;
		}
	}

	return -1;
}

/* mmnif_rxbuff_free() : the opposite to mmnif_rxbuff_alloc() a from the receiver
 * already processed chunk of memory is freed so that it can be allocated again
 */
static void mmnif_rxbuff_free(void)
{
	mmnif_t *mmnif = mmnif_dev->state;
	volatile mm_rx_buffer_t *b = mmnif->rx_buff;
	uint32_t i, j;
	uint32_t rpos;
	uint8_t flags;

	flags = irq_nested_disable();
	islelock_lock(isle_locks + (isle+1));
	rpos = b->dread;

	for (i = 0, j = rpos; i < MMNIF_MAX_DESCRIPTORS; i++)
	{
		j = (j + i) % MMNIF_MAX_DESCRIPTORS;
		if (b->desc_table[j].stat == MMNIF_STATUS_PROC)
		{
			b->dcount++;
			b->dread = (b->dread + 1) % MMNIF_MAX_DESCRIPTORS;
			b->desc_table[j].stat = MMNIF_STATUS_FREE;
			if (b->tail > b->head)
			{
				b->head += b->desc_table[j].len;
			} else {
				if ((b->desc_table[(j + 1) % MMNIF_MAX_DESCRIPTORS].stat != MMNIF_STATUS_FREE)
				    && (b->desc_table[j].addr > b->desc_table[(j + 1) % MMNIF_MAX_DESCRIPTORS].addr))
				{
					b->head = 0;
				} else {
					b->head += b->desc_table[j].len;
				}
			}
		} else
			break;
	}

	islelock_unlock(isle_locks + (isle+1));
	irq_nested_enable(flags);
}

/*
 * Transmid a packet (called by the lwip)
 */
static err_t mmnif_tx(struct netif *netif, struct pbuf *p)
{
	mmnif_t *mmnif = netif->state;
	size_t write_address;
	uint32_t i;
	struct pbuf *q;		/* interator */
	uint32_t dest_ip = mmnif_get_destination(netif, p);

	/* check for over/underflow */
 	if (BUILTIN_EXPECT((p->tot_len < 20 /* IP header size */) || (p->tot_len > 1536), 0)) {
		LOG_ERROR("mmnif_tx: illegal packet length %d => drop\n", p->tot_len);
		goto drop_packet;
	}

	/* check destination ip */
	if (BUILTIN_EXPECT((dest_ip < 1) || (dest_ip > MAX_ISLE), 0)) {
		LOG_ERROR("mmnif_tx: invalid destination IP %d => drop\n", dest_ip);
		goto drop_packet;
	}

	spinlock_irqsave_lock(&locallock); // only one core should call our islelock

	/* allocate memory for the packet in the remote buffer */
realloc:
	write_address = mmnif_rxbuff_alloc(dest_ip, p->tot_len);
	if (!write_address)
	{
		LOG_DEBUG("mmnif_tx(): concurrency");

		PAUSE;
		goto realloc;
	}

	for (q = p, i = 0; q != 0; q = q->next)
	{
		memcpy((char*) write_address + i, q->payload, q->len);
		i += q->len;
	}

	if (mmnif_commit_packet(dest_ip, write_address))
	{
		LOG_WARNING("mmnif_tx(): packet somehow lost during commit\n");
	}

#ifdef DEBUG_MMNIF_PACKET
//      LOG_INFO("\n SEND %p with length: %d\n",(char*)heap_start_address + (dest_ip -1)*mpb_size + pos * 1792,p->tot_len +2);
//      hex_dump(p->tot_len, p->payload);
#endif

	/* just gather some stats */
	LINK_STATS_INC(link.xmit);
	mmnif->stats.tx++;
	mmnif->stats.tx_bytes += p->tot_len;

	spinlock_irqsave_unlock(&locallock);

	mmnif_trigger_irq(dest_ip);

	return ERR_OK;

drop_packet:
	/* drop packet for one or another reason
	 */
	LOG_ERROR("mmnif_tx(): packet dropped");

	LINK_STATS_INC(link.drop);
	mmnif->stats.tx_err++;

	return ERR_IF;
}

/* mmnif_link_layer(): wrapper function called by ip_output()
 * adding all needed headers for the link layer
 * because we have no link layer and everything is reliable we don't need
 * to add anything so we just pass it to our tx function
 */
static err_t mmnif_link_layer(struct netif *netif, struct pbuf *q, ip_addr_t * ipaddr)
{
	return netif->linkoutput(netif, q);
}

/*
 * Init the device (called from lwip)
 * It's invoked in netif_add
 */
err_t mmnif_init(struct netif *netif)
{
	mmnif_t *mmnif = NULL;
	int num = 0;
	int err;
	uint32_t nodes = possible_isles + 1;
	size_t flags;

	LOG_INFO("Initialize mmnif\n");

	mmnif_dev = netif;

	/* Alloc and clear memory for the device struct
	 */
	mmnif = kmalloc(sizeof(mmnif_t));
	if (BUILTIN_EXPECT(!mmnif, 0))
	{
		LOG_ERROR("mmnif init():out of memory\n");
		goto out;
	}
	memset(mmnif, 0x00, sizeof(mmnif_t));

	/* Alloc and clear shared memory for rx_buff
	 */
	if (BUILTIN_EXPECT(header_size < sizeof(mm_rx_buffer_t), 0))
	{
		LOG_ERROR("mmnif init(): header_size is too small\n");
		goto out;
	}

	if (BUILTIN_EXPECT(heap_size < MMNIF_RX_BUFFERLEN, 0))
	{
		LOG_ERROR("mmnif init(): heap_size is too small\n");
		goto out;
	}
	LOG_INFO("mmnif_init() : size of mm_rx_buffer_t : %d\n", sizeof(mm_rx_buffer_t));

	if (BUILTIN_EXPECT(!header_phy_start_address || !header_phy_start_address || !phy_isle_locks, 0))
	{
		LOG_ERROR("mmnif init(): invalid heap or header address\n");
		goto out;
	}

	if (BUILTIN_EXPECT(!header_start_address, 0))
	{
		LOG_ERROR("mmnif init(): vma_alloc failed\n");
		goto out;
	}

	err = vma_add((size_t)header_start_address, PAGE_FLOOR((size_t)header_start_address + ((nodes * header_size) >> PAGE_BITS)), VMA_READ|VMA_WRITE|VMA_CACHEABLE);
	if (BUILTIN_EXPECT(err, 0)) {
		LOG_ERROR("mmnif init(): vma_add failed for header_start_address %p\n", header_start_address);
		goto out;
	}

	// protect mmnif shared segments by the NX flag
	flags = PG_RW|PG_GLOBAL;
	if (has_nx())
		flags |= PG_XD;

	// map physical address in the virtual address space
	err = page_map((size_t) header_start_address, (size_t) header_phy_start_address, (nodes * header_size) >> PAGE_BITS, flags);
	if (BUILTIN_EXPECT(err, 0)) {
		LOG_ERROR("mmnif init(): page_map failed\n");
		goto out;
	}

	LOG_INFO("map header %p at %p\n", header_phy_start_address, header_start_address);
	mmnif->rx_buff = (mm_rx_buffer_t *) (header_start_address + header_size * (isle+1));

	if (BUILTIN_EXPECT(!heap_start_address, 0)) {
		LOG_ERROR("mmnif init(): vma_alloc failed\n");
		goto out;
	}

	err = vma_add((size_t)heap_start_address, PAGE_FLOOR((size_t)heap_start_address + ((nodes * heap_size) >> PAGE_BITS)), VMA_READ|VMA_WRITE|VMA_CACHEABLE);
	if (BUILTIN_EXPECT(!heap_start_address, 0))
	{
		LOG_ERROR("mmnif init(): vma_add failed for heap_start_address %p\n", heap_start_address);
		goto out;
	}

	// map physical address in the virtual address space
	err = page_map((size_t) heap_start_address, (size_t) heap_phy_start_address, (nodes * heap_size) >> PAGE_BITS, flags);
	if (BUILTIN_EXPECT(err, 0)) {
		LOG_ERROR("mmnif init(): page_map failed\n");
		goto out;
	}

	// map physical address in the virtual address space
	LOG_INFO("map heap %p at %p\n", heap_phy_start_address, heap_start_address);
	mmnif->rx_heap = (uint8_t*) heap_start_address + heap_size * (isle+1);

	memset((void*)mmnif->rx_buff, 0x00, header_size);
	memset((void*)mmnif->rx_heap, 0x00, heap_size);

	isle_locks = (islelock_t*) vma_alloc(((nodes + 1) * sizeof(islelock_t) + PAGE_SIZE - 1) & ~(PAGE_SIZE - 1), VMA_READ|VMA_WRITE|VMA_CACHEABLE);
	if (BUILTIN_EXPECT(!isle_locks, 0)) {
		LOG_ERROR("mmnif init(): vma_alloc failed\n");
		goto out;
	}

	// map physical address in the virtual address space
	err = page_map((size_t) isle_locks, (size_t) phy_isle_locks, (((nodes+1) * sizeof(islelock_t) + PAGE_SIZE - 1) & ~(PAGE_SIZE - 1)) >> PAGE_BITS, flags);
	if (BUILTIN_EXPECT(err, 0)) {
		LOG_ERROR("mmnif init(): page_map failed\n");
		goto out;
	}
	LOG_INFO("map isle_locks %p at %p\n", phy_isle_locks, isle_locks);

	/* set initial values
	 */
	mmnif->rx_buff->dcount = MMNIF_MAX_DESCRIPTORS;

	/* init the lock's for the hdr
	 */
	spinlock_irqsave_init(&locallock);

	/* init the sems for communication art
	 */
	sem_init(&mmnif->com_poll, 0);

	/* pass the device state to lwip */
	netif->state = mmnif;
	mmnif_dev = netif;

	/* administrative details */
	netif->name[0] = 'm';
	netif->name[1] = 'm';
	netif->num = num;
	num++;

	/* downward functions */
	netif->output = (netif_output_fn) mmnif_link_layer;

	/* there is no special link layer just the ip layer */
	netif->linkoutput = mmnif_tx;

	/* maximum transfer unit */
	netif->mtu = 1500;

	/* broadcast capability, keep all default flags */
	//netif->flags |= NETIF_FLAG_BROADCAST;
	/* set link up */
	netif->flags |= NETIF_FLAG_LINK_UP;

	/* hardware address length */
	netif->hwaddr_len = 0;

	// set interrupt handler
	irq_install_handler(MMNIF_IRQ, mmnif_irqhandler);

	LOG_INFO("mmnif init complete\n");

	return ERR_OK;

out:
	if (!mmnif)
		kfree(mmnif);

	header_start_address = NULL;
	heap_start_address = NULL;

	return ERR_MEM;
}

/*
 * Receive a packet : recieve, pack it up and pass over to higher levels
 */
static void mmnif_rx(struct netif *netif)
{
	mmnif_t *mmnif = netif->state;
	volatile mm_rx_buffer_t *b = mmnif->rx_buff;
	uint16_t length = 0;
	struct pbuf *p;
	struct pbuf *q;
	char *packet = NULL;
	uint32_t i, j, flags;
	uint8_t rdesc;
	err_t err = ERR_OK;

anotherpacket:
	flags = irq_nested_disable();
	rdesc = 0xFF;

	/* check if this call to mmnif_rx makes any sense
	 */
	if (b->desc_table[b->dread].stat == MMNIF_STATUS_FREE)
	{
		mmnif->check_in_progress = 0;
		irq_nested_enable(flags);
		return;
	}

	/* search the packet whose transmission is finished
	 */
	for (i = 0, j = b->dread; i < MMNIF_MAX_DESCRIPTORS; i++)
	{
		if (b->desc_table[(j + i) % MMNIF_MAX_DESCRIPTORS].stat == MMNIF_STATUS_RDY)
		{
			rdesc = (j + i) % MMNIF_MAX_DESCRIPTORS;
			b->desc_table[rdesc].stat = MMNIF_STATUS_INPROC;
			packet = (char *)b->desc_table[rdesc].addr;
			length = b->desc_table[rdesc].len;
			break;
		}

		if (b->desc_table[(j + i) % MMNIF_MAX_DESCRIPTORS].stat == MMNIF_STATUS_FREE)
		{
			mmnif->check_in_progress = 0;
			irq_nested_enable(flags);
			return;
		}
	}

	irq_nested_enable(flags);

	/* if there is no packet finished we encountered a random error
	 */
	if (rdesc == 0xFF)
		goto out;

	/* If length is zero return silently
	 */
	if (BUILTIN_EXPECT(length == 0, 0))
	{
		LOG_ERROR("mmnif_rx(): empty packet error\n");
		goto out;
	}

	/* check for over/underflow */
        if (BUILTIN_EXPECT((length < 20 /* IP header size */) || (length > 1536), 0))
	{
		LOG_ERROR("mmnif_rx(): illegal packet length %d => drop the packet\n", length);
		goto drop_packet;
	}

	/* From now on there is a real packet and it
	 * has to be worked on
	 */
#ifdef DEBUG_MMNIF_PACKET
	LOG_INFO("\n RECIEVED - %p with legth: %d\n", packet, length);
	hex_dump(length, packet);
#endif

	/* Build the pbuf for the packet so the lwip
	 * and other higher layer can handle it
	 */
	p = pbuf_alloc(PBUF_RAW, length, PBUF_POOL);
	if (BUILTIN_EXPECT(!p, 0))
	{
		LOG_ERROR("mmnif_rx(): low on mem - packet dropped\n");
		goto drop_packet;
	}

	/* copy packet to pbuf structure going through linked list */
	for (q = p, i = 0; q != NULL; q = q->next)
	{
		memcpy((uint8_t *) q->payload, packet + i, q->len);
		i += q->len;
	}

	/* indicate that the copy process is done and the packet can be freed
	 * note that we did not lock here because we are the only one editing this value
	 */
	mmnif->rx_buff->desc_table[rdesc].stat = MMNIF_STATUS_PROC;
	mb();

	/* everything is copied to a new buffer so it's save to release
	 * the old one for new incoming packets
	 */
	mmnif_rxbuff_free();

	/*
	 * This function is called in the context of the tcpip thread.
	 * Therefore, we are able to call directly the input functions.
	 */
	if ((err = mmnif_dev->input(p, mmnif_dev)) != ERR_OK)
	{
		LOG_ERROR("mmnif_rx: IP input error\n");
		pbuf_free(p);
	}

	/* gather some stats and leave the rx handler */
	LINK_STATS_INC(link.xmit);
	mmnif->stats.rx++;
	mmnif->stats.rx_bytes += p->tot_len;
	goto anotherpacket;

drop_packet:
	/* TODO: error handling */
	LINK_STATS_INC(link.drop);
	mmnif->stats.rx_err++;
out:
	mmnif->check_in_progress = 0;
	return;
}

/* mmnif_irqhandler():
 * handles the incomint interrupts
 */
static void mmnif_irqhandler(struct state* s)
{
	mmnif_t *mmnif;

	/* return if mmnif_dev is not yet initialized */
	if (!mmnif_dev)
	{
		LOG_ERROR("mmnif_irqhandler(): the driver is not initialized yet\n");
		return;
	}

	mmnif = (mmnif_t *) mmnif_dev->state;
	if (!mmnif->check_in_progress) {
#if LWIP_TCPIP_CORE_LOCKING_INPUT
		mmnif->check_in_progress = 1;
		mmnif_rx(mmnif_dev);
#else
		if (tcpip_callback_with_block((tcpip_callback_fn) mmnif_rx, (void*) mmnif_dev, 0) == ERR_OK) {
			mmnif->check_in_progress = 1;
		} else {
			LOG_ERROR("mmnif_handler: unable to send a poll request to the tcpip thread\n");
		}
#endif
	}
}

/*
 * close the interface should be called by kernel to close this interface and release resources
 * Note: it's temporarly empty. Support will be added.
 */
err_t mmnif_shutdown(void)
{
	err_t err;

	if (!mmnif_dev) {
		LOG_ERROR("mmnif_shutdown(): you closed the device before it was properly initialized -.-* \n");
		return ERR_MEM;
	}

	err = netifapi_netif_set_down(mmnif_dev);

	mmnif_dev = NULL;

	return err;
}
