/* 
 * Copyright 2012 Stefan Lankes, Chair for Operating Systems,
 *                               RWTH Aachen University
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
 */

#include <hermit/stddef.h>
#include <hermit/stdio.h>
#include <hermit/string.h>
#include <hermit/processor.h>
#include <hermit/mailbox.h>
#include <hermit/logging.h>
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
#include <net/e1000.h>

#define RX_BUF_LEN      (2048)
#define TX_BUF_LEN      (1792)

#define INT_MASK		(E1000_IMS_RXO|E1000_IMS_RXT0|E1000_IMS_RXDMT0|E1000_IMS_RXSEQ|E1000_IMS_LSC)
#define INT_MASK_NO_RX		(E1000_IMS_LSC)

typedef struct {
	char *vendor_str;
	char *device_str;
	uint32_t vendor;
	uint32_t device;
} board_t;

static board_t board_tbl[] = 
{
	{"Intel", "Intel E1000 (82542)", 0x8086, 0x1000},
	{"Intel", "Intel E1000 (82543GC FIBER)", 0x8086, 0x1001},
	{"Intel", "Intel E1000 (82543GC COPPER)", 0x8086, 0x1004},
	{"Intel", "Intel E1000 (82544EI COPPER)", 0x8086, 0x1008},
	{"Intel", "Intel E1000 (82544EI FIBER)", 0x8086, 0x1009},
	{"Intel", "Intel E1000 (82544GC COPPER)", 0x8086, 0x100C},
	{"Intel", "Intel E1000 (82544GC LOM)", 0x8086, 0x100D},
	{"Intel", "Intel E1000 (82540EM)", 0x8086, 0x100E},	
	{"Intel", "Intel E1000 (82540EM LOM)", 0x8086, 0x1015},
	{"Intel", "Intel E1000 (82540EP LOM)", 0x8086, 0x1016},
	{"Intel", "Intel E1000 (82540EP)", 0x8086, 0x1017},
	{"Intel", "Intel E1000 (82540EP LP)", 0x8086, 0x101E},
	{"Intel", "Intel E1000 (82545EM COPPER)", 0x8086, 0x100F},
	{"Intel", "Intel E1000 (82545EM FIBER)", 0x8086, 0x1011},
	{"Intel", "Intel E1000 (82545GM COPPER)", 0x8086, 0x1026},
	{"Intel", "Intel E1000 (82545GM FIBER)", 0x8086, 0x1027},
	{"Intel", "Intel E1000 (82545GM SERDES)", 0x8086, 0x1028},
	{"Intel", "Intel E1000 (82546EB COPPER)", 0x8086, 0x1010},
	{"Intel", "Intel E1000 (82546EB FIBER)", 0x8086, 0x1012},
	{"Intel", "Intel E1000 (82546EB QUAD COPPER)", 0x8086, 0x101D},
	//{"Intel", "Intel E1000 (82541EI)", 0x8086, 0x1013},
	//{"Intel", "Intel E1000 (82541EI MOBILE)", 0x8086, 0x1018},
	//{"Intel", "Intel E1000 (82541ER LOM)", 0x8086, 0x1014},
	//{"Intel", "Intel E1000 (82541ER)", 0x8086, 0x1078},
	{"Intel", "Intel E1000 (82547GI)", 0x8086, 0x1075},
	{"Intel", "Intel E1000 (82541GI)", 0x8086, 0x1076},
	{"Intel", "Intel E1000 (82541GI MOBILE)", 0x8086, 0x1077},
	{"Intel", "Intel E1000 (82541GI LF)", 0x8086, 0x107C},
	{"Intel", "Intel E1000 (82546GB COPPER)", 0x8086, 0x1079},
	{"Intel", "Intel E1000 (82546GB FIBER)", 0x8086, 0x107A},
	{"Intel", "Intel E1000 (82546GB SERDES)", 0x8086, 0x107B},
	{"Intel", "Intel E1000 (82546GB PCIE)", 0x8086, 0x108A},
	{"Intel", "Intel E1000 (82546GB QUAD COPPER)", 0x8086, 0x1099},
	//{"Intel", "Intel E1000 (82547EI)", 0x8086, 0x1019},
	//{"Intel", "Intel E1000 (82547EI_MOBILE)", 0x8086, 0x101A},
	{"Intel", "Intel E1000 (82546GB QUAD COPPER KSP3)", 0x8086, 0x10B5},
	{NULL,},
};

static struct netif* mynetif = NULL;

static inline uint32_t e1000_read(volatile uint8_t* base, uint32_t off)
{
#if 1
	uint32_t ret;

	asm volatile ("movl (%1), %0" : "=r"(ret) : "r"(base+off));

	return ret;
#else
	return *((volatile uint32_t*) (base+off));
#endif
}

static inline void e1000_write(volatile uint8_t* base, uint32_t off, uint32_t value)
{
	*((volatile uint32_t*) (base+off)) = value;
}

static inline void e1000_flush(volatile uint8_t* base)
{
	e1000_read(base, E1000_STATUS);
}

#if 1
static uint16_t eeprom_read(volatile uint8_t* base, uint8_t addr)
{
	uint16_t data;
	uint32_t tmp;

	e1000_write(base, E1000_EERD, 1 | ((uint32_t)(addr) << 8));

	while(!((tmp = e1000_read(base, E1000_EERD)) & (1 << 4))) 
		udelay(1);

	data = (uint16_t)((tmp >> 16) & 0xFFFF);

	return data;
}
#else
// Only for 82541xx and 82547GI/EI
static uint16_t eeprom_read(uint8_t* base, uint8_t addr)
{
	uint16_t data;
	uint32_t tmp;

	e1000_write(base, E1000_EERD, 1 | ((uint32_t)(addr) << 2));

	while(!((tmp = e1000_read(base, E1000_EERD)) & (1 << 1))) 
		udelay(1);

	data = (uint16_t)((tmp >> 16) & 0xFFFF);

	return data;
}
#endif

/*
 * @return error code
 * - ERR_OK: packet transferred to hardware
 * - ERR_CONN: no link or link failure
 * - ERR_IF: could not transfer to link (hardware buffer full?)
 */
static err_t e1000if_output(struct netif* netif, struct pbuf* p)
{
	e1000if_t* e1000if = netif->state;
	uint32_t i;
	struct pbuf *q;

	if (BUILTIN_EXPECT((p->tot_len > 1792) || (p->tot_len > TX_BUF_LEN), 0)) {
		LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_output: packet is longer than 1792 bytes\n"));
		return ERR_IF;
	}

	if (!(e1000if->tx_desc[e1000if->tx_tail].status & 0xF)) {
		LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_output: %i already inuse\n", e1000if->tx_tail));
		return ERR_IF;
	}

#if ETH_PAD_SIZE
	pbuf_header(p, -ETH_PAD_SIZE); /* drop the padding word */
#endif

	/*
	 * q traverses through linked list of pbuf's
	 * This list MUST consist of a single packet ONLY
	 */
	for (q = p, i = 0; q != 0; q = q->next) {
		memcpy((void*) ((size_t) e1000if->tx_buffers + e1000if->tx_tail*TX_BUF_LEN + i), q->payload, q->len);
		i += q->len;
	}

	e1000if->tx_desc[e1000if->tx_tail].length = p->tot_len;
	e1000if->tx_desc[e1000if->tx_tail].status = 0;
	e1000if->tx_desc[e1000if->tx_tail].cmd = (1 << 3) | 3;

	// update the tail so the hardware knows it's ready
	e1000if->tx_tail = (e1000if->tx_tail + 1) % NUM_TX_DESCRIPTORS;
	e1000_write(e1000if->bar0, E1000_TDT, e1000if->tx_tail);	

#if ETH_PAD_SIZE
	pbuf_header(p, ETH_PAD_SIZE); /* reclaim the padding word */
#endif

	LINK_STATS_INC(link.xmit);

	return ERR_OK;
}

static void e1000_rx_inthandler(struct netif* netif)
{
	e1000if_t* e1000if = netif->state;
	struct pbuf *p = NULL;
	struct pbuf* q;
	uint16_t length, i;

	while(e1000if->rx_desc[e1000if->rx_tail].status & (1 << 0))
	{
		if (!(e1000if->rx_desc[e1000if->rx_tail].status & (1 << 1))) {
			LINK_STATS_INC(link.drop);
			goto no_eop; // currently, we ignore packets without EOP flag
		}

		length = e1000if->rx_desc[e1000if->rx_tail].length;

		if (!e1000if->rx_desc[e1000if->rx_tail].errors) {
#if ETH_PAD_SIZE
			length += ETH_PAD_SIZE; /* allow room for Ethernet padding */
#endif

			p = pbuf_alloc(PBUF_RAW, length, PBUF_POOL);
			if (p) {
#if ETH_PAD_SIZE
				pbuf_header(p, -ETH_PAD_SIZE); /* drop the padding word */
#endif
				for (q=p, i=0; q!=NULL; q=q->next) {
					memcpy((uint8_t*) q->payload, (void*) ((size_t) e1000if->rx_buffers + e1000if->rx_tail*RX_BUF_LEN + i), q->len);
					i += q->len;
				}
#if ETH_PAD_SIZE
				pbuf_header(p, ETH_PAD_SIZE); /* reclaim the padding word */
#endif
				LINK_STATS_INC(link.recv);

				// forward packet to LwIP
				netif->input(p, netif);
			} else {
				LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_rx_inthandler: not enough memory!\n"));
				LINK_STATS_INC(link.memerr);
				LINK_STATS_INC(link.drop);
			}
		} else {
			LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_rx_inthandler: RX errors (0x%x)\n", e1000if->rx_desc[e1000if->rx_tail].errors));
			LINK_STATS_INC(link.drop);
		}

no_eop:		
		e1000if->rx_desc[e1000if->rx_tail].status = 0;

		// update tail and write the value to the device
		e1000if->rx_tail = (e1000if->rx_tail + 1) % NUM_RX_DESCRIPTORS;
		e1000_write(e1000if->bar0, E1000_RDT, e1000if->rx_tail);
	}

	e1000if->polling = 0;
	// enable all known interrupts
	e1000_write(e1000if->bar0, E1000_IMS, INT_MASK);
	e1000_flush(e1000if->bar0);
}

/* this function is called in the context of the tcpip thread or the irq handler (by using NO_SYS) */
static void e1000if_poll(void* ctx)
{
	e1000_rx_inthandler(mynetif);
}

static void e1000if_handler(struct state* s)
{
	e1000if_t* e1000if = mynetif->state;
	uint32_t icr;

	// disable all interrupts
	e1000_write(e1000if->bar0, E1000_IMC, INT_MASK|0xFFFE0000);
	e1000_flush(e1000if->bar0);

	// read the pending interrupt status
	icr = e1000_read(e1000if->bar0, E1000_ICR);

	// ignore tx success stuff
	icr &= ~3;

	// LINK STATUS CHANGE
	if (icr & E1000_ICR_LSC)
	{
		icr &= ~E1000_ICR_LSC;
		LWIP_DEBUGF(NETIF_DEBUG, ("e1000if: Link status change (TODO)\n"));
	}

	if (icr &  (E1000_ICR_RXT0|E1000_ICR_RXDMT0|E1000_ICR_RXO)) {
		icr &= ~(E1000_ICR_RXT0|E1000_ICR_RXDMT0|E1000_ICR_RXO);

		if (!e1000if->polling) {
#if NO_SYS
			e1000if_poll(NULL);
#else
			if (tcpip_callback_with_block(e1000if_poll, NULL, 0) == ERR_OK) {
				e1000if->polling = 1;
			} else {
				LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_handler: unable to send a poll request to the tcpip thread\n"));
			}
#endif
 		}
	}

	if (e1000if->polling) // now, the tcpip thread will check for incoming messages
		e1000_write(e1000if->bar0, E1000_IMS, INT_MASK_NO_RX);
	else
		e1000_write(e1000if->bar0, E1000_IMS, INT_MASK); // enable interrupts
	e1000_flush(e1000if->bar0);

	if (icr & 0x1FFFF) {
		LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_handler: unhandled interrupt #%u received! (0x%x)\n", e1000if->irq, icr));
	}
}

err_t e1000if_init(struct netif* netif)
{
	pci_info_t pci_info;
	e1000if_t* e1000if = NULL;
	uint32_t tmp32;
	uint16_t tmp16, speed, cold = 0x40;
	uint8_t tmp8, is64bit, mem_type, prefetch;
	static uint8_t num = 0;
	
	LWIP_ASSERT("netif != NULL", (netif != NULL));

	tmp8 = 0;
	while (board_tbl[tmp8].vendor_str) {
		if (pci_get_device_info(board_tbl[tmp8].vendor, board_tbl[tmp8].device, &pci_info, 1) == 0)
			break;
		tmp8++;
	}

	if (!board_tbl[tmp8].vendor_str)
		return ERR_ARG;

	mem_type = pci_info.base[0] & 0x1;
	is64bit = pci_info.base[0] & 0x6 ? 1 : 0;
	prefetch = pci_info.base[0] & 0x8 ? 1 : 0;

	if (mem_type) {
		LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: IO space is currently not supported!\n"));
		return ERR_ARG;
	}

	if (is64bit) {
		LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: 64bit mode is currently not supported!\n"));
		return ERR_ARG;
	}

	e1000if = kmalloc(sizeof(e1000if_t));
	if (!e1000if) {
		LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: out of memory\n"));
		return ERR_MEM;
	}
	memset(e1000if, 0x00, sizeof(e1000if_t));

	netif->state = e1000if;
	mynetif = netif;

	e1000if->bar0 = (uint8_t*) vma_alloc(PAGE_FLOOR(pci_info.size[0]), VMA_READ|VMA_WRITE);
	if (BUILTIN_EXPECT(!e1000if->bar0, 0))
		goto oom;

	int ret = page_map((size_t)e1000if->bar0, PAGE_CEIL(pci_info.base[0]), PAGE_FLOOR(pci_info.size[0]) >> PAGE_BITS, PG_GLOBAL|PG_RW|PG_PCD);
	if (BUILTIN_EXPECT(ret, 0))
		goto oom;

	// reset device
	e1000_write(e1000if->bar0, E1000_CTRL, E1000_CTRL_RST);
	e1000_flush(e1000if->bar0);
	/* Wait for reset to complete */
	udelay(10);

	e1000if->irq = pci_info.irq;
	e1000if->rx_desc = page_alloc(NUM_RX_DESCRIPTORS*sizeof(rx_desc_t), VMA_READ|VMA_WRITE);
	if (BUILTIN_EXPECT(!e1000if->rx_desc, 0))
		goto oom;
	memset((void*) e1000if->rx_desc, 0x00, NUM_RX_DESCRIPTORS*sizeof(rx_desc_t));
	e1000if->tx_desc = page_alloc(NUM_TX_DESCRIPTORS*sizeof(tx_desc_t), VMA_READ|VMA_WRITE);
	if (BUILTIN_EXPECT(!e1000if->tx_desc, 0))
		goto oom;
	memset((void*) e1000if->tx_desc, 0x00, NUM_TX_DESCRIPTORS*sizeof(tx_desc_t));

	LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: Found %s at mmio 0x%x (size 0x%x), irq %u\n", board_tbl[tmp8].device_str, 
		pci_info.base[0] & ~0xF, pci_info.size[0], e1000if->irq));
	//LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: Map iobase to %p\n", e1000if->bar0));
	LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: is64bit %u, prefetch %u\n", is64bit, prefetch));

	/* hardware address length */
        netif->hwaddr_len = ETHARP_HWADDR_LEN;

	// determine the mac address of this card
	for (tmp8=0; tmp8<ETHARP_HWADDR_LEN; tmp8+=2) {
		tmp16 = eeprom_read(e1000if->bar0, tmp8 / 2);
		netif->hwaddr[tmp8] = (tmp16 & 0xFF);
		netif->hwaddr[tmp8+1] = (tmp16 >> 8) & 0xFF;
	}

	e1000if->tx_buffers = page_alloc(NUM_TX_DESCRIPTORS*TX_BUF_LEN, VMA_READ|VMA_WRITE);
	if (BUILTIN_EXPECT(!e1000if->tx_buffers, 0))
		goto oom;
	memset((void*) e1000if->tx_buffers, 0x00, NUM_TX_DESCRIPTORS*TX_BUF_LEN);
	for(tmp32=0; tmp32 < NUM_TX_DESCRIPTORS; tmp32++) {
		e1000if->tx_desc[tmp32].addr = virt_to_phys((size_t)e1000if->tx_buffers + tmp32*TX_BUF_LEN);
                e1000if->tx_desc[tmp32].status = 1;
	}

	//LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: add TX ring buffer %p (viraddr %p)\n", virt_to_phys((size_t)e1000if->tx_desc), e1000if->tx_desc));

	/* General configuration */
	tmp32 = e1000_read(e1000if->bar0, E1000_CTRL);
	tmp32 &= ~(E1000_CTRL_VME|E1000_CTRL_FD|E1000_CTRL_ILOS|E1000_CTRL_PHY_RST|E1000_CTRL_LRST|E1000_CTRL_FRCSPD);
	e1000_write(e1000if->bar0, E1000_CTRL, tmp32 | E1000_CTRL_SLU | E1000_CTRL_ASDE);
	e1000_flush(e1000if->bar0);
	LOG_INFO("e1000if_init: Device Control Register 0x%x\n", e1000_read(e1000if->bar0, E1000_CTRL));

	/* make sure transmits are disabled while setting up the descriptors */
	tmp32 = e1000_read(e1000if->bar0, E1000_TCTL);
	e1000_write(e1000if->bar0, E1000_TCTL, tmp32 & ~E1000_TCTL_EN);
	e1000_flush(e1000if->bar0);

	// setup the transmit descriptor ring buffer
	e1000_write(e1000if->bar0, E1000_TDBAL, (uint32_t)((uint64_t)virt_to_phys((size_t)e1000if->tx_desc) & 0xFFFFFFFF));
	e1000_write(e1000if->bar0, E1000_TDBAH, (uint32_t)((uint64_t)virt_to_phys((size_t)e1000if->tx_desc) >> 32));
	//LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: TDBAH/TDBAL = 0x%x:0x%x\n", e1000_read(e1000if->bar0, E1000_TDBAH), e1000_read(e1000if->bar0, E1000_TDBAL)));

	// transmit buffer length; NUM_TX_DESCRIPTORS 16-byte descriptors
	e1000_write(e1000if->bar0, E1000_TDLEN , (uint32_t)(NUM_TX_DESCRIPTORS * sizeof(tx_desc_t)));
	
	// setup head and tail pointers
	e1000_write(e1000if->bar0, E1000_TDH, 0);
	e1000_write(e1000if->bar0, E1000_TDT, 0);
	e1000if->tx_tail = 0;

	tmp32 = e1000_read(e1000if->bar0, E1000_STATUS);
	if (tmp32 & E1000_STATUS_SPEED_1000)
		speed = 1000;
	else if (tmp32 & E1000_STATUS_SPEED_100)
		speed = 100;
	else
		speed = 10;

	if ((!(tmp32 & E1000_STATUS_FD)) && (speed == 1000))
		cold = 0x200;
	LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: speed = %u mbps\n", speed));
	LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: Full-Duplex %u\n", tmp32 & E1000_STATUS_FD));

	// set the transmit control register (padshortpackets)
	e1000_write(e1000if->bar0, E1000_TCTL, (E1000_TCTL_EN | E1000_TCTL_PSP | (cold << 12) | (0x10 << 4)));
	e1000_flush(e1000if->bar0);

	// set IEEE 802.3 standard IPG value
	e1000_write(e1000if->bar0, E1000_TIPG, (6 << 20) | (8 << 10) | 10);

	// set MAC address
	for(tmp8=0; tmp8<4; tmp8++)
		((uint8_t*) &tmp32)[tmp8] = netif->hwaddr[tmp8];
	e1000_write(e1000if->bar0, E1000_RA, tmp32);
	tmp32 = 0;
	for(tmp8=0; tmp8<2; tmp8++)
		((uint8_t*) &tmp32)[tmp8] = netif->hwaddr[tmp8+4];
	e1000_write(e1000if->bar0, E1000_RA+4, tmp32 | (1 << 31)); // set also AV bit to check incoming packets 

	/* Zero out the other receive addresses. */
	for (tmp8=1; tmp8<16; tmp8++) {
		e1000_write(e1000if->bar0, E1000_RA+8*tmp8, 0);
		e1000_write(e1000if->bar0, E1000_RA+8*tmp8+4, 0);
	}

	LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: MAC address "));
	tmp32 = e1000_read(e1000if->bar0, E1000_RA);
	for(tmp8=0; tmp8<4; tmp8++) {
		LWIP_DEBUGF(NETIF_DEBUG, ("%02x ", ((uint8_t*) &tmp32)[tmp8]));
	}
	tmp32 = e1000_read(e1000if->bar0, E1000_RA+4);
	for(tmp8=0; tmp8<2; tmp8++) {
		LWIP_DEBUGF(NETIF_DEBUG, ("%02x ", ((uint8_t*) &tmp32)[tmp8]));
	}
	LWIP_DEBUGF(NETIF_DEBUG, ("\n"));
	e1000_flush(e1000if->bar0);

	// set multicast table to 0
	for(tmp8=0; tmp8<128; tmp8++ )
		e1000_write(e1000if->bar0, E1000_MTA + (tmp8 * 4), 0);
	e1000_flush(e1000if->bar0);

	// set IRQ handler
	irq_install_handler(e1000if->irq+32, e1000if_handler);

	/* make sure receives are disabled while setting up the descriptors */
	tmp32 = e1000_read(e1000if->bar0, E1000_RCTL);
	e1000_write(e1000if->bar0, E1000_RCTL, tmp32 & ~E1000_RCTL_EN);
	e1000_flush(e1000if->bar0);

	// clear IMS & IMC registers
	e1000_write(e1000if->bar0, E1000_IMS, 0xFFFF);
	e1000_flush(e1000if->bar0);
	e1000_write(e1000if->bar0, E1000_IMC, 0xFFFF);
	e1000_flush(e1000if->bar0);

	// enable all interrupts (and clear existing pending ones)
	e1000_write(e1000if->bar0, E1000_IMS, INT_MASK);
	e1000_flush(e1000if->bar0);
	e1000_read(e1000if->bar0, E1000_ICR);

	LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: Interrupt Mask is set to 0x%x\n", e1000_read(e1000if->bar0, E1000_IMS)));

	//LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: add RX ring buffer %p (viraddr %p)\n", virt_to_phys((size_t)e1000if->rx_desc), e1000if->rx_desc));

	e1000if->rx_buffers = page_alloc(NUM_RX_DESCRIPTORS*RX_BUF_LEN, VMA_READ|VMA_WRITE);
	if (BUILTIN_EXPECT(!e1000if->rx_buffers, 0))
		goto oom;
	memset(e1000if->rx_buffers, 0x00, NUM_RX_DESCRIPTORS*RX_BUF_LEN);
	for(tmp32=0; tmp32 < NUM_RX_DESCRIPTORS; tmp32++)
		e1000if->rx_desc[tmp32].addr = virt_to_phys((size_t)e1000if->rx_buffers + tmp32*RX_BUF_LEN);

	// setup the receive descriptor ring buffer
	e1000_write(e1000if->bar0, E1000_RDBAH, (uint32_t)((uint64_t)virt_to_phys((size_t)e1000if->rx_desc) >> 32));
	e1000_write(e1000if->bar0, E1000_RDBAL, (uint32_t)((uint64_t)virt_to_phys((size_t)e1000if->rx_desc) & 0xFFFFFFFF));

        // receive buffer length; NUM_RX_DESCRIPTORS 16-byte descriptors
        e1000_write(e1000if->bar0, E1000_RDLEN , (uint32_t)(NUM_RX_DESCRIPTORS * sizeof(rx_desc_t)));

        // setup head and tail pointers
        e1000_write(e1000if->bar0, E1000_RDH, 0);
        e1000_write(e1000if->bar0, E1000_RDT, 0);
        e1000if->rx_tail = 0;

	// set the receieve control register
	e1000_write(e1000if->bar0, E1000_RCTL, (E1000_RCTL_EN|/*E1000_RCTL_LPE|*/E1000_RCTL_LBM_NO|E1000_RCTL_BAM|E1000_RCTL_SZ_2048|
						E1000_RCTL_SECRC|E1000_RCTL_RDMTS_HALF|E1000_RCTL_MO_0/*|E1000_RCTL_UPE|E1000_RCTL_MPE*/));
	e1000_flush(e1000if->bar0);

	LWIP_DEBUGF(NETIF_DEBUG, ("e1000if_init: status = 0x%x\n", e1000_read(e1000if->bar0, E1000_STATUS)));

	/*
	 * Initialize the snmp variables and counters inside the struct netif.
	 * The last argument should be replaced with your link speed, in units
	 * of bits per second.
	 */
	NETIF_INIT_SNMP(netif, snmp_ifType_ethernet_csmacd, speed);

	/* administrative details */
	netif->name[0] = 'e';
	netif->name[1] = 'n';
	netif->num = num;
	num++;
	/* downward functions */
	netif->output = etharp_output;
	netif->linkoutput = e1000if_output;
	/* maximum transfer unit */
	netif->mtu = 1500;
	/* broadcast capability */
	netif->flags |= NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_IGMP | NETIF_FLAG_LINK_UP | NETIF_FLAG_MLD6;

	e1000if->ethaddr = (struct eth_addr *)netif->hwaddr;

#if LWIP_IPV6
	netif->output_ip6 = ethip6_output;
	netif_create_ip6_linklocal_address(netif, 1);
	netif->ip6_autoconfig_enabled = 1;
#endif

	return ERR_OK;

oom:
	if (e1000if)
	{
		if (e1000if->rx_desc)
			page_free((void*) e1000if->rx_desc, NUM_RX_DESCRIPTORS*sizeof(rx_desc_t));
		if (e1000if->tx_desc)
			page_free((void*) e1000if->tx_desc, NUM_TX_DESCRIPTORS*sizeof(tx_desc_t));
		if (e1000if->tx_buffers)
			page_free(e1000if->tx_buffers, NUM_TX_DESCRIPTORS*TX_BUF_LEN);
		if (e1000if->rx_buffers)
			page_free(e1000if->rx_buffers, NUM_RX_DESCRIPTORS*RX_BUF_LEN);
		if (e1000if->bar0) {
			e1000_write(e1000if->bar0, E1000_CTRL, E1000_CTRL_RST);

			// TODO: unmap e1000if->bar0
		}

		irq_uninstall_handler(e1000if->irq+32);

		kfree(e1000if);
	}

	memset(netif, 0x00, sizeof(struct netif));
	mynetif = NULL;

	return ERR_MEM;
}
