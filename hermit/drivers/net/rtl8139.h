/* 
 * Copyright 2010 Stefan Lankes, Chair for Operating Systems,
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
 * This code based mostly on the online manual http://www.lowlevel.eu/wiki/RTL8139
 */

#ifndef __NET_RTL8139_H__
#define __NET_RTL8139_H__

#include <hermit/stddef.h>
#include <hermit/spinlock.h>

// the registers are at the following places
#define IDR0    0x0		// the ethernet ID (6bytes)
#define MAR0    0x8		// Multicast (8 bytes)
#define TSD0    0x10		// transmit status of each descriptor (4bytes/descriptor) (C mode)
#define DTCCR   0x10		// Dump Tally Counter Command Register (C+ mode)
#define TSAD0   0x20		// transmit start address of descriptor 0 (4byte, C mode, 4 byte alignment)
#define TSAD1   0x24		// transmit start address of descriptor 1 (4byte, C mode, 4 byte alignment)
#define TNPDS   0x20		// transmit normal priority descriptors start address (8bytes, C+ mode, 256 byte-align)
#define TSAD2   0x28		// transmit start address of descriptor 2 (4byte, C mode, 4 byte alignment)
#define TSAD3   0x2c		// transmit start address of descriptor 3 (4byte, C mode, 4 byte alignment)
#define THPDS   0x28		// transmit high priority descriptors start address (8byte, C+ mode, 256 byte-align)
#define RBSTART 0x30		// recieve buffer start address (C mode, 4 byte alignment)
#define ERBCR   0x34		// early recieve byte count (2byte)
#define ERSR    0x36		// early recieve state register (1byte)
#define CR      0x37		// command register (1byte)
#define CAPR    0x38		// current address of packet read (2byte, C mode, initial value 0xFFF0)
#define CBR     0x3a		// current buffer address , total recieved byte-count in the Rx buffer (2byte, C mode, initial value 0x0000)
#define IMR     0x3c		// interrupt mask register (2byte)
#define ISR     0x3e		// interrupt status register (2byte)
#define TCR     0x40		// transmit config register (4byte)
#define RCR     0x44		// receive config register (4byte)
#define TCTR    0x48		// timer count register, write any value and it will reset the count, and count from zero (4byte)
#define MPC     0x4C		// missed packet count , number of packets ignored due to RX overflow, 24-bit, write a value to reset (4byte, top is void)
#define CR9346  0x50		// command register for 93C46 (93C56) (1byte)
#define CONFIG0 0x51		// config register 0 (1byte)
#define CONFIG1 0x52		// config register 1 (1byte)
#define TIMINT  0x54		// timer interrupt register , the timeout bit will be set when the value of this == value of TCTR (4byte, when 0 does nothing)
#define MSR     0x58		// media status register (1byte)
#define CONFIG3 0x59		// config register 3 (1byte)
#define CONFIG4 0x5a		// config register 4 (1byte)
#define MULINT  0x5c		// multiple interrupt select (4byte)
#define RERID   0x5e		// revision ID (C+ = 0x10)
#define TSAD    0x60		// transmit status of ALL descriptors (2byte, C mode)
#define BMCR    0x62		// basic mode control register (2byte)
#define BMSR    0x64		// basic mode status register (2byte)
#define ANAR    0x66		// Auto-negotiation advertisement register (2byte)
#define ANLPAR  0x68		// Auto-negotiation link partner register (2byte)
#define ANER    0x6a		// Auto-negotiation expansion register (2byte)
#define DIS     0x6c		// disconnected counter (2byte)
#define FCSC    0x6e		// false carrier sense counter (2byte)
#define NWAYTR  0x70		// N-way test register (2byte)
#define REC     0x72		// RX_ER (counts valid packets) counter (2byte)
#define CSCR    0x74		// CS config register (2byte)
#define PHYS1P  0x78		// PHY parameter 1 (2byte)
#define TWP     0x7c		// twister parameter (2byte)
#define PHYS2P  0x80		// PHY parameter 2
// some power managment registers are here
#define FLASH   0xD4		// flash memory read/write (4byte)
#define CONFIG5 0xD8		// config register 5 (1byte)
#define TPPoll  0xD9		// transmit priority polling (1byte, C+ mode)
#define CPCR    0xE0		// C+ command register (2byte, C+ mode)
#define RDSAR   0xE4		// C+ receive descriptor start address (4byte, C+ mode, 256 byte alignment)
#define ETTR   0xEC		// C+ early transmit threshold (1byte, C+ mode)
// some cardbus only stuff goes here
#define MIIR    0xFC		// MII register (Auto-detect or MII mode only)

// Command Register
#define CR_RST		0x10	// Reset, set to 1 to invoke S/W reset, held to 1 while resetting
#define CR_RE		0x08	// Reciever Enable, enables receiving
#define CR_TE		0x04	// Transmitter Enable, enables transmitting
#define CR_BUFE		0x01	// Rx buffer is empty

// Transmit Configuration Register
#define TCR_HWVERID	0x7CC00000	// mask for hw version ID's
#define TCR_HWOFFSET	22
#define TCR_IFG		0x3000000	// interframe gap time
#define TCR_LBK1	0x40000	// loopback test
#define TCR_LBK0	0x20000	// loopback test
#define TCR_CRC		0x10000	// append CRC (card adds CRC if 1)
#define TCR_MXDMA2	0x400	// max dma burst
#define TCR_MXDMA1	0x200	// max dma burst
#define TCR_MXDMA0	0x100	// max dma burst
#define TCR_TXRR	0xF0	// Tx retry count, 0 = 16 else retries TXRR * 16 + 16 times
#define TCR_CLRABT	0x01	// Clear abort, attempt retransmit (when in abort state)

// Media Status Register
#define MSR_TXFCE	0x80	// Tx Flow Control enabled
#define MSR_RXFCE	0x40	// Rx Flow Control enabled
#define MSR_AS		0x10	// Auxilary status
#define MSR_SPEED	0x8	// set if currently talking on 10mbps network, clear if 100mbps
#define MSR_LINKB	0x4	// Link Bad ?
#define MSR_TXPF	0x2	// Transmit Pause flag
#define MSR_RXPF	0x1	// Recieve Pause flag

// Basic mode control register
#define BMCR_RESET	0x8000	// set the status and control of PHY to default
#define BMCR_SPD100	(1 << 13) // 100 MBit
#define BMCR_SPD1000	(1 << 6)  // 1000 MBit
#define BMCR_ANE	0x1000	// enable N-way autonegotiation (ignore above if set)
#define BMCR_RAN	0x400	// restart auto-negotiation
#define BMCR_DUPLEX	0x200	// Duplex mode, generally a value of 1 means full-duplex

// Receive Configuration Register
#define RCR_ERTH3       0x8000000	// early Rx Threshold 0
#define RCR_ERTH2       0x4000000	// early Rx Threshold 1
#define RCR_ERTH1       0x2000000	// early Rx Threshold 2
#define RCR_ERTH0       0x1000000	// early Rx Threshold 3
#define RCR_MRINT       0x20000	// Multiple Early interrupt, (enable to make interrupts happen early, yuk)
#define RCR_RER8        0x10000	// Receive Error Packets larger than 8 bytes
#define RCR_RXFTH2      0x8000	// Rx Fifo threshold 0
#define RCR_RXFTH1      0x4000	// Rx Fifo threshold 1 (set to 110 and it will send to system when 1024bytes have been gathered)
#define RCR_RXFTH0      0x2000	// Rx Fifo threshold 2 (set all these to 1, and it wont FIFO till the full packet is ready)
#define RCR_RBLEN1      0x1000	// Rx Buffer length 0
#define RCR_RBLEN0      0x800	// Rx Buffer length 1 (C mode, 11 = 64kb, 10 = 32k, 01 = 16k, 00 = 8k)
#define RCR_MXDMA2      0x400	// Max DMA burst size 0
#define RCR_MXDMA1      0x200	// Max DMA burst size 1
#define RCR_MXDMA0      0x100	// Max DMA burst size 2
#define RCR_WRAP        0x80	// (void if buffer size = 64k, C mode, wrap to beginning of Rx buffer if we hit the end)
#define RCR_EEPROMSEL   0x40	// EEPROM type (0 = 9346, 1 = 9356)
#define RCR_AER         0x20	// Accept Error Packets (do we accept bad packets ?)
#define RCR_AR          0x10	// Accept runt packets (accept packets that are too small ?)
#define RCR_AB          0x08	// Accept Broadcast packets (accept broadcasts ?)
#define RCR_AM          0x04	// Accept multicast ?
#define RCR_APM         0x02	// Accept Physical matches (accept packets sent to our mac ?)
#define RCR_AAP         0x01	// Accept packets with a physical address ?

// Interrupt Status/Mask Register
// Bits in IMR enable/disable interrupts for specific events
// Bits in ISR indicate the status of the card
#define ISR_SERR        0x8000	// System error interrupt
#define ISR_TUN         0x4000	// time out interrupt
#define ISR_SWInt       0x100	// Software interrupt
#define ISR_TDU         0x80	// Tx Descriptor unavailable
#define ISR_FIFOOVW     0x40	// Rx Fifo overflow
#define ISR_PUN         0x20	// Packet underrun/link change
#define ISR_RXOVW       0x10	// Rx overflow/Rx Descriptor unavailable
#define ISR_TER         0x08	// Tx Error
#define ISR_TOK         0x04	// Tx OK
#define ISR_RER         0x02	// Rx Error
#define ISR_ROK         0x01	// Rx OK
#define R39_INTERRUPT_MASK      0x7f

// CR9346 Command register
#define CR9346_EEM1     0x80	// determine the operating mode
#define CR9346_EEM0     0x40	// 00 = Normal, 01 = Auto-load, 10 = Programming, 11 = Config, Register write enabled
#define CR9346_EECS     0x8	// status of EECS
#define CR9346_EESK     0x4	// status of EESK
#define CR9346_EEDI     0x2	// status of EEDI
#define CR9346_EEDO     0x1	// status of EEDO

// CONFIG1 stuff
#define CONFIG1_LEDS    0xC0	// leds status
#define CONFIG1_DVRLOAD 0x20	// is the driver loaded ?
#define CONFIG1_LWACT   0x10	// lanwake mode
#define CONFIG1_MEMMAP  0x8	// Memory mapping enabled ?
#define CONFIG1_IOMAP   0x4	// IO map enabled ?
#define CONFIG1_VPD     0x2	// enable the virtal product data
#define CONFIG1_PMEn    0x1	// Power Managment Enable

// CONFIG3 stuff
#define CONFIG3_GNT     0x80	// Grant Select enable
#define CONFIG3_PARM    0x40	// Parameter auto-load enabled ?
#define CONFIG3_MAGIC   0x20	// Magic packet ?
#define CONFIG3_LINKUP  0x10	// wake computer when link goes up ?
#define CONFIG3_CardB   0x08	// Card Bus stuff enabled ?
#define CONFIG3_CLKRUN  0x04	// enable CLKRUN ?
#define CONFIG3_FRE     0x02	// Function registers enabled ? (cardbus only)
#define CONFIG3_FBBE    0x01	// fast back to back enabled ?

// CONFIG4 stuff ?
#define CONFIG4_RXFAC   0x80	// Clear Rx Fifo overflow, when enabled the card will clear FIFO overflow automatically
#define CONFIG4_AnaOff  0x40	// Analogue power off ?
#define CONFIG4_LWF     0x20	// Long wake-up frame
#define CONFIG4_LWPME   0x10	// LANWAKE vs PMEB
#define CONFIG4_LWPTN   0x04	// Lan wake pattern ?
#define CONFIG4_PBWAKE  0x01	// pre-boot wakeup

//Transmit Status of Descriptor0-3 (C mode only)
#define TSD_CRS		(1 << 31)	// carrier sense lost (during packet transmission)
#define TSD_TABT	(1 << 30)	// transmission abort
#define TSD_OWC		(1 << 29)	// out of window collision
#define TSD_CDH		(1 << 28)	// CD Heart beat (Cleared in 100Mb mode)
#define TSD_NCC		0xF000000	// Number of collisions counted (during transmission)
#define TSD_EARTH	0x3F0000	// threshold to begin transmission (0 = 8bytes, 1->2^6 = * 32bytes)
#define TSD_TOK		(1 << 15)	// Transmission OK, successful
#define TSD_TUN		(1 << 14)	// Transmission FIFO underrun
#define TSD_OWN		(1 << 13)	// Tx DMA operation finished (driver must set to 0 when TBC is written)
#define TSD_SIZE	0x1fff	// Descriptor size, the total size in bytes of data to send (max 1792)

/*
 * Helper struct to hold private data used to operate your ethernet interface.
 */
typedef struct rtl1839if {
	struct eth_addr *ethaddr;
	/* Add whatever per-interface state that is needed here. */
	uint8_t*	tx_buffer[4];
	uint8_t*	rx_buffer;
	uint32_t	iobase;
	uint32_t	tx_queue;
	uint32_t	tx_complete;
	uint16_t	rx_pos;
	uint8_t		tx_inuse[4];
	uint8_t		irq;
	volatile uint8_t polling;
} rtl1839if_t;

/*
 * Initialize the network driver for the RealTek RTL8139 family
 */
err_t rtl8139if_init(struct netif* netif);

#endif
