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
 *  SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 */

#ifndef __NET_E1000_H__
#define __NET_E1000_H__

#include <hermit/stddef.h>
#include <hermit/spinlock.h>

#ifdef USE_E1000

#define NUM_RX_DESCRIPTORS	64
#define NUM_TX_DESCRIPTORS	64

#define E1000_CTRL	0x00000	/* Device Control - RW */
#define E1000_CTRL_DUP	0x00004	/* Device Control Duplicate (Shadow) - RW */
#define E1000_STATUS	0x00008	/* Device Status - RO */
#define E1000_EECD	0x00010	/* EEPROM/Flash Control - RW */
#define E1000_EERD	0x00014	/* EEPROM Read - RW */
#define E1000_CTRL_EXT	0x00018	/* Extended Device Control - RW */
#define E1000_ICR	0x000C0	/* Interrupt Cause Read - R/clr */
#define E1000_ITR	0x000C4	/* Interrupt Throttling Rate - RW */
#define E1000_ICS	0x000C8	/* Interrupt Cause Set - WO */
#define E1000_IMS	0x000D0	/* Interrupt Mask Set - RW */
#define E1000_IMC	0x000D8	/* Interrupt Mask Clear - WO */
#define E1000_IAM	0x000E0	/* Interrupt Acknowledge Auto Mask */
#define E1000_RCTL	0x00100	/* RX Control - RW */
#define E1000_TCTL	0x00400	/* TX Control - RW */
#define E1000_TIPG	0x00410	/* TX Inter-packet gap -RW */
#define E1000_RDBAL	0x02800	/* RX Descriptor Base Address Low - RW */
#define E1000_RDBAH	0x02804	/* RX Descriptor Base Address High - RW */
#define E1000_RDLEN	0x02808	/* RX Descriptor Length - RW */
#define E1000_RDH	0x02810	/* RX Descriptor Head - RW */
#define E1000_RDT	0x02818	/* RX Descriptor Tail - RW */
#define E1000_TDBAL	0x03800	/* TX Descriptor Base Address Low - RW */
#define E1000_TDBAH	0x03804	/* TX Descriptor Base Address High - RW */
#define E1000_TDLEN	0x03808 /* TX Descriptor Length - RW */
#define E1000_TDH	0x03810 /* TX Descriptor Head - RW */
#define E1000_TDT	0x03818 /* TX Descripotr Tail - RW */
#define E1000_MTA	0x05200	/* Multicast Table Array - RW Array */
#define E1000_RA	0x05400	/* Receive Address - RW Array */

/* Device Control */
#define E1000_CTRL_FD		0x00000001  /* Full duplex.0=half; 1=full */
#define E1000_CTRL_BEM		0x00000002  /* Endian Mode.0=little,1=big */
#define E1000_CTRL_PRIOR	0x00000004  /* Priority on PCI. 0=rx,1=fair */
#define E1000_CTRL_GIO_MASTER_DISABLE 0x00000004 /*Blocks new Master requests */
#define E1000_CTRL_LRST		0x00000008  /* Link reset. 0=normal,1=reset */
#define E1000_CTRL_TME		0x00000010  /* Test mode. 0=normal,1=test */
#define E1000_CTRL_SLE		0x00000020  /* Serial Link on 0=dis,1=en */
#define E1000_CTRL_ASDE		0x00000020  /* Auto-speed detect enable */
#define E1000_CTRL_SLU		0x00000040  /* Set link up (Force Link) */
#define E1000_CTRL_ILOS		0x00000080  /* Invert Loss-Of Signal */
#define E1000_CTRL_SPD_SEL	0x00000300  /* Speed Select Mask */
#define E1000_CTRL_SPD_10	0x00000000  /* Force 10Mb */
#define E1000_CTRL_SPD_100	0x00000100  /* Force 100Mb */
#define E1000_CTRL_SPD_1000	0x00000200  /* Force 1Gb */
#define E1000_CTRL_BEM32	0x00000400  /* Big Endian 32 mode */
#define E1000_CTRL_FRCSPD	0x00000800  /* Force Speed */
#define E1000_CTRL_FRCDPX	0x00001000  /* Force Duplex */
#define E1000_CTRL_D_UD_EN	0x00002000  /* Dock/Undock enable */
#define E1000_CTRL_D_UD_POLARITY	0x00004000 /* Defined polarity of Dock/Undock indication in SDP[0] */
#define E1000_CTRL_FORCE_PHY_RESET	0x00008000 /* Reset both PHY ports, through PHYRST_N pin */
#define E1000_CTRL_EXT_LINK_EN	0x00010000  /* enable link status from external LINK_0 and LINK_1 pins */
#define E1000_CTRL_SWDPIN0	0x00040000  /* SWDPIN 0 value */
#define E1000_CTRL_SWDPIN1	0x00080000  /* SWDPIN 1 value */
#define E1000_CTRL_SWDPIN2	0x00100000  /* SWDPIN 2 value */
#define E1000_CTRL_SWDPIN3	0x00200000  /* SWDPIN 3 value */
#define E1000_CTRL_SWDPIO0	0x00400000  /* SWDPIN 0 Input or output */
#define E1000_CTRL_SWDPIO1	0x00800000  /* SWDPIN 1 input or output */
#define E1000_CTRL_SWDPIO2	0x01000000  /* SWDPIN 2 input or output */
#define E1000_CTRL_SWDPIO3	0x02000000  /* SWDPIN 3 input or output */
#define E1000_CTRL_RST		0x04000000  /* Global reset */
#define E1000_CTRL_RFCE		0x08000000  /* Receive Flow Control enable */
#define E1000_CTRL_TFCE		0x10000000  /* Transmit flow control enable */
#define E1000_CTRL_RTE		0x20000000  /* Routing tag enable */
#define E1000_CTRL_VME		0x40000000  /* IEEE VLAN mode enable */
#define E1000_CTRL_PHY_RST	0x80000000  /* PHY Reset */
#define E1000_CTRL_SW2FW_INT	0x02000000  /* Initiate an interrupt to manageability engine */

/* Device Status */
#define E1000_STATUS_FD		0x00000001      /* Full duplex.0=half,1=full */
#define E1000_STATUS_LU		0x00000002      /* Link up.0=no,1=link */
#define E1000_STATUS_FUNC_MASK	0x0000000C      /* PCI Function Mask */
#define E1000_STATUS_FUNC_SHIFT	2
#define E1000_STATUS_FUNC_0	0x00000000      /* Function 0 */
#define E1000_STATUS_FUNC_1	0x00000004      /* Function 1 */
#define E1000_STATUS_TXOFF	0x00000010      /* transmission paused */
#define E1000_STATUS_TBIMODE	0x00000020      /* TBI mode */
#define E1000_STATUS_SPEED_MASK	0x000000C0
#define E1000_STATUS_SPEED_10	0x00000000      /* Speed 10Mb/s */
#define E1000_STATUS_SPEED_100	0x00000040      /* Speed 100Mb/s */
#define E1000_STATUS_SPEED_1000	0x00000080      /* Speed 1000Mb/s */
#define E1000_STATUS_LAN_INIT_DONE 0x00000200   /* Lan Init Completion
                                                   by EEPROM/Flash */
#define E1000_STATUS_ASDV	0x00000300      /* Auto speed detect value */
#define E1000_STATUS_DOCK_CI	0x00000800      /* Change in Dock/Undock state. Clear on write '0'. */
#define E1000_STATUS_GIO_MASTER_ENABLE 0x00080000 /* Status of Master requests. */
#define E1000_STATUS_MTXCKOK	0x00000400      /* MTX clock running OK */
#define E1000_STATUS_PCI66	0x00000800      /* In 66Mhz slot */
#define E1000_STATUS_BUS64	0x00001000      /* In 64 bit slot */
#define E1000_STATUS_PCIX_MODE	0x00002000      /* PCI-X mode */
#define E1000_STATUS_PCIX_SPEED	0x0000C000      /* PCI-X bus speed */
#define E1000_STATUS_BMC_SKU_0	0x00100000 /* BMC USB redirect disabled */
#define E1000_STATUS_BMC_SKU_1	0x00200000 /* BMC SRAM disabled */
#define E1000_STATUS_BMC_SKU_2	0x00400000 /* BMC SDRAM disabled */
#define E1000_STATUS_BMC_CRYPTO	0x00800000 /* BMC crypto disabled */
#define E1000_STATUS_BMC_LITE	0x01000000 /* BMC external code execution disabled */
#define E1000_STATUS_RGMII_ENABLE 0x02000000 /* RGMII disabled */
#define E1000_STATUS_FUSE_8	0x04000000
#define E1000_STATUS_FUSE_9	0x08000000
#define E1000_STATUS_SERDES0_DIS 0x10000000 /* SERDES disabled on port 0 */
#define E1000_STATUS_SERDES1_DIS 0x20000000 /* SERDES disabled on port 1 */

/* Transmit Control */
#define E1000_TCTL_RST		0x00000001    /* software reset */
#define E1000_TCTL_EN		0x00000002    /* enable tx */
#define E1000_TCTL_BCE		0x00000004    /* busy check enable */
#define E1000_TCTL_PSP		0x00000008    /* pad short packets */
#define E1000_TCTL_CT		0x00000ff0    /* collision threshold */
#define E1000_TCTL_COLD		0x003ff000    /* collision distance */
#define E1000_TCTL_SWXOFF	0x00400000    /* SW Xoff transmission */
#define E1000_TCTL_PBE		0x00800000    /* Packet Burst Enable */
#define E1000_TCTL_RTLC		0x01000000    /* Re-transmit on late collision */
#define E1000_TCTL_NRTU		0x02000000    /* No Re-transmit on underrun */
#define E1000_TCTL_MULR		0x10000000    /* Multiple request support */

/* Receive Control */
#define E1000_RCTL_RST		0x00000001	/* Software reset */
#define E1000_RCTL_EN		0x00000002	/* enable */
#define E1000_RCTL_SBP		0x00000004	/* store bad packet */
#define E1000_RCTL_UPE		0x00000008	/* unicast promiscuous enable */
#define E1000_RCTL_MPE		0x00000010	/* multicast promiscuous enable */
#define E1000_RCTL_LPE		0x00000020	/* long packet enable */
#define E1000_RCTL_LBM_NO	0x00000000	/* no loopback mode */
#define E1000_RCTL_LBM_MAC	0x00000040	/* MAC loopback mode */
#define E1000_RCTL_LBM_SLP	0x00000080	/* serial link loopback mode */
#define E1000_RCTL_LBM_TCVR	0x000000C0	/* tcvr loopback mode */
#define E1000_RCTL_DTYP_MASK	0x00000C00	/* Descriptor type mask */
#define E1000_RCTL_DTYP_PS	0x00000400	/* Packet Split descriptor */
#define E1000_RCTL_RDMTS_HALF	0x00000000	/* rx desc min threshold size */
#define E1000_RCTL_RDMTS_QUAT	0x00000100	/* rx desc min threshold size */
#define E1000_RCTL_RDMTS_EIGTH	0x00000200	/* rx desc min threshold size */
#define E1000_RCTL_MO_SHIFT	12		/* multicast offset shift */
#define E1000_RCTL_MO_0		0x00000000	/* multicast offset 11:0 */
#define E1000_RCTL_MO_1		0x00001000	/* multicast offset 12:1 */
#define E1000_RCTL_MO_2		0x00002000	/* multicast offset 13:2 */
#define E1000_RCTL_MO_3		0x00003000	/* multicast offset 15:4 */
#define E1000_RCTL_MDR		0x00004000	/* multicast desc ring 0 */
#define E1000_RCTL_BAM		0x00008000	/* broadcast enable */
/* these buffer sizes are valid if E1000_RCTL_BSEX is 0 */
#define E1000_RCTL_SZ_2048	0x00000000	/* rx buffer size 2048 */
#define E1000_RCTL_SZ_1024	0x00010000	/* rx buffer size 1024 */
#define E1000_RCTL_SZ_512	0x00020000	/* rx buffer size 512 */
#define E1000_RCTL_SZ_256	0x00030000	/* rx buffer size 256 */
/* these buffer sizes are valid if E1000_RCTL_BSEX is 1 */
#define E1000_RCTL_SZ_16384	0x00010000	/* rx buffer size 16384 */
#define E1000_RCTL_SZ_8192	0x00020000	/* rx buffer size 8192 */
#define E1000_RCTL_SZ_4096	0x00030000	/* rx buffer size 4096 */
#define E1000_RCTL_VFE		0x00040000	/* vlan filter enable */
#define E1000_RCTL_CFIEN	0x00080000	/* canonical form enable */
#define E1000_RCTL_CFI		0x00100000	/* canonical form indicator */
#define E1000_RCTL_DPF		0x00400000	/* discard pause frames */
#define E1000_RCTL_PMCF		0x00800000	/* pass MAC control frames */
#define E1000_RCTL_BSEX		0x02000000	/* Buffer size extension */
#define E1000_RCTL_SECRC	0x04000000	/* Strip Ethernet CRC */
#define E1000_RCTL_FLXBUF_MASK	0x78000000	/* Flexible buffer size */
#define E1000_RCTL_FLXBUF_SHIFT	27		/* Flexible buffer shift */

/* Interrupt Cause Read */
#define E1000_ICR_TXDW		0x00000001 /* Transmit desc written back */
#define E1000_ICR_TXQE		0x00000002 /* Transmit Queue empty */
#define E1000_ICR_LSC		0x00000004 /* Link Status Change */
#define E1000_ICR_RXSEQ		0x00000008 /* rx sequence error */
#define E1000_ICR_RXDMT0	0x00000010 /* rx desc min. threshold (0) */
#define E1000_ICR_RXO		0x00000040 /* rx overrun */
#define E1000_ICR_RXT0		0x00000080 /* rx timer intr (ring 0) */
#define E1000_ICR_MDAC		0x00000200 /* MDIO access complete */
#define E1000_ICR_RXCFG		0x00000400 /* RX /c/ ordered set */
#define E1000_ICR_GPI_EN0	0x00000800 /* GP Int 0 */
#define E1000_ICR_GPI_EN1	0x00001000 /* GP Int 1 */
#define E1000_ICR_GPI_EN2	0x00002000 /* GP Int 2 */
#define E1000_ICR_GPI_EN3	0x00004000 /* GP Int 3 */
#define E1000_ICR_TXD_LOW	0x00008000
#define E1000_ICR_SRPD		0x00010000
#define E1000_ICR_ACK		0x00020000 /* Receive Ack frame */
#define E1000_ICR_MNG		0x00040000 /* Manageability event */
#define E1000_ICR_DOCK		0x00080000 /* Dock/Undock */
#define E1000_ICR_INT_ASSERTED	0x80000000 /* If this bit asserted, the driver should claim the interrupt */
#define E1000_ICR_RXD_FIFO_PAR0	0x00100000 /* queue 0 Rx descriptor FIFO parity error */
#define E1000_ICR_TXD_FIFO_PAR0	0x00200000 /* queue 0 Tx descriptor FIFO parity error */
#define E1000_ICR_HOST_ARB_PAR	0x00400000 /* host arb read buffer parity error */
#define E1000_ICR_PB_PAR	0x00800000 /* packet buffer parity error */
#define E1000_ICR_RXD_FIFO_PAR1	0x01000000 /* queue 1 Rx descriptor FIFO parity error */
#define E1000_ICR_TXD_FIFO_PAR1	0x02000000 /* queue 1 Tx descriptor FIFO parity error */
#define E1000_ICR_ALL_PARITY	0x03F00000 /* all parity error bits */
#define E1000_ICR_DSW		0x00000020 /* FW changed the status of DISSW bit in the FWSM */
#define E1000_ICR_PHYINT	0x00001000 /* LAN connected device generates an interrupt */
#define E1000_ICR_EPRST		0x00100000 /* ME handware reset occurs */

/* Interrupt Mask Set */
#define E1000_IMS_TXDW		E1000_ICR_TXDW      /* Transmit desc written back */
#define E1000_IMS_TXQE		E1000_ICR_TXQE      /* Transmit Queue empty */
#define E1000_IMS_LSC		E1000_ICR_LSC       /* Link Status Change */
#define E1000_IMS_RXSEQ		E1000_ICR_RXSEQ     /* rx sequence error */
#define E1000_IMS_RXDMT0	E1000_ICR_RXDMT0    /* rx desc min. threshold */
#define E1000_IMS_RXO		E1000_ICR_RXO       /* rx overrun */
#define E1000_IMS_RXT0		E1000_ICR_RXT0      /* rx timer intr */
#define E1000_IMS_MDAC		E1000_ICR_MDAC      /* MDIO access complete */
#define E1000_IMS_RXCFG		E1000_ICR_RXCFG     /* RX /c/ ordered set */
#define E1000_IMS_GPI_EN0	E1000_ICR_GPI_EN0   /* GP Int 0 */
#define E1000_IMS_GPI_EN1	E1000_ICR_GPI_EN1   /* GP Int 1 */
#define E1000_IMS_GPI_EN2	E1000_ICR_GPI_EN2   /* GP Int 2 */
#define E1000_IMS_GPI_EN3	E1000_ICR_GPI_EN3   /* GP Int 3 */
#define E1000_IMS_TXD_LOW	E1000_ICR_TXD_LOW
#define E1000_IMS_SRPD		E1000_ICR_SRPD
#define E1000_IMS_ACK		E1000_ICR_ACK       /* Receive Ack frame */
#define E1000_IMS_MNG		E1000_ICR_MNG       /* Manageability event */
#define E1000_IMS_DOCK		E1000_ICR_DOCK      /* Dock/Undock */
#define E1000_IMS_RXD_FIFO_PAR0	E1000_ICR_RXD_FIFO_PAR0 /* queue 0 Rx descriptor FIFO parity error */
#define E1000_IMS_TXD_FIFO_PAR0	E1000_ICR_TXD_FIFO_PAR0 /* queue 0 Tx descriptor FIFO parity error */
#define E1000_IMS_HOST_ARB_PAR	E1000_ICR_HOST_ARB_PAR  /* host arb read buffer parity error */
#define E1000_IMS_PB_PAR	E1000_ICR_PB_PAR        /* packet buffer parity error */
#define E1000_IMS_RXD_FIFO_PAR1	E1000_ICR_RXD_FIFO_PAR1 /* queue 1 Rx descriptor FIFO parity error */
#define E1000_IMS_TXD_FIFO_PAR1	E1000_ICR_TXD_FIFO_PAR1 /* queue 1 Tx descriptor FIFO parity error */
#define E1000_IMS_DSW		E1000_ICR_DSW
#define E1000_IMS_PHYINT	E1000_ICR_PHYINT
#define E1000_IMS_EPRST		E1000_ICR_EPRST

/* Interrupt Mask Clear */
#define E1000_IMC_TXDW		E1000_ICR_TXDW      /* Transmit desc written back */
#define E1000_IMC_TXQE		E1000_ICR_TXQE      /* Transmit Queue empty */
#define E1000_IMC_LSC		E1000_ICR_LSC       /* Link Status Change */
#define E1000_IMC_RXSEQ		E1000_ICR_RXSEQ     /* rx sequence error */
#define E1000_IMC_RXDMT0	E1000_ICR_RXDMT0    /* rx desc min. threshold */
#define E1000_IMC_RXO		E1000_ICR_RXO       /* rx overrun */
#define E1000_IMC_RXT0		E1000_ICR_RXT0      /* rx timer intr */
#define E1000_IMC_MDAC		E1000_ICR_MDAC      /* MDIO access complete */
#define E1000_IMC_RXCFG		E1000_ICR_RXCFG     /* RX /c/ ordered set */
#define E1000_IMC_GPI_EN0	E1000_ICR_GPI_EN0   /* GP Int 0 */
#define E1000_IMC_GPI_EN1	E1000_ICR_GPI_EN1   /* GP Int 1 */
#define E1000_IMC_GPI_EN2	E1000_ICR_GPI_EN2   /* GP Int 2 */
#define E1000_IMC_GPI_EN3	E1000_ICR_GPI_EN3   /* GP Int 3 */
#define E1000_IMC_TXD_LOW	E1000_ICR_TXD_LOW
#define E1000_IMC_SRPD		E1000_ICR_SRPD
#define E1000_IMC_ACK		E1000_ICR_ACK       /* Receive Ack frame */
#define E1000_IMC_MNG		E1000_ICR_MNG       /* Manageability event */
#define E1000_IMC_DOCK		E1000_ICR_DOCK      /* Dock/Undock */
#define E1000_IMC_RXD_FIFO_PAR0	E1000_ICR_RXD_FIFO_PAR0 /* queue 0 Rx descriptor FIFO parity error */
#define E1000_IMC_TXD_FIFO_PAR0	E1000_ICR_TXD_FIFO_PAR0 /* queue 0 Tx descriptor FIFO parity error */
#define E1000_IMC_HOST_ARB_PAR	E1000_ICR_HOST_ARB_PAR  /* host arb read buffer parity error */
#define E1000_IMC_PB_PAR	E1000_ICR_PB_PAR        /* packet buffer parity error */
#define E1000_IMC_RXD_FIFO_PAR1	E1000_ICR_RXD_FIFO_PAR1 /* queue 1 Rx descriptor FIFO parity error */
#define E1000_IMC_TXD_FIFO_PAR1	E1000_ICR_TXD_FIFO_PAR1 /* queue 1 Tx descriptor FIFO parity error */
#define E1000_IMC_DSW		E1000_ICR_DSW
#define E1000_IMC_PHYINT	E1000_ICR_PHYINT
#define E1000_IMC_EPRST		E1000_ICR_EPRST

// TX and RX descriptor
typedef struct __attribute__((packed))
{
	uint64_t 	addr;
	uint16_t 	length;
	uint8_t		cso;
	uint8_t 	cmd;
	uint8_t		status;
	uint8_t 	css;
	uint16_t 	special;
} tx_desc_t;

typedef struct __attribute__((packed))
{
	uint64_t	addr;
	uint16_t	length;
	uint16_t	checksum;
	uint8_t 	status;
	uint8_t		errors;
	uint16_t	special;
} rx_desc_t;

/*
 * Helper struct to hold private data used to operate your ethernet interface.
 */
typedef struct e1000if {
	struct eth_addr *ethaddr;
	/* Add whatever per-interface state that is needed here. */
	volatile uint8_t*	bar0;
	uint8_t*		tx_buffers;
	uint8_t*		rx_buffers;
	volatile tx_desc_t*	tx_desc; // transmit descriptor buffer
	uint16_t		tx_tail;
	volatile rx_desc_t*	rx_desc; // receive descriptor buffer
	uint16_t		rx_tail;
	uint8_t			irq;
	volatile uint8_t	polling;
} e1000if_t;

/*
 * Initialize the network driver for the RealTek RTL8139 family
 */
err_t e1000if_init(struct netif* netif);

#endif

#endif
