// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

// The driver based on the online manual http://www.lowlevel.eu/wiki/RTL8139

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use core::convert::TryInto;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::{mem, str};

use smoltcp::iface::{EthernetInterfaceBuilder, NeighborCache, Routes};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};

use crate::arch::x86_64::kernel::apic;
use crate::arch::x86_64::kernel::irq::*;
use crate::arch::x86_64::kernel::pci;
use crate::arch::x86_64::kernel::percore::core_scheduler;
use crate::arch::x86_64::mm::paging::virt_to_phys;
use crate::drivers::net::{networkd, NETWORK_TASK_ID, NET_SEM};
use crate::scheduler;
use crate::x86::io::*;

/// size of the receive buffer
const RX_BUF_LEN: usize = 8192;
/// size of the send buffer
const TX_BUF_LEN: usize = 4096;

/// the ethernet ID (6bytes) => MAC address
const IDR0: u16 = 0x0;
/// transmit status of each descriptor (4bytes/descriptor) (C mode)
const TSD0: u16 = 0x10;
/// transmit start address of descriptor 0 (4byte, C mode, 4 byte alignment)
const TSAD0: u16 = 0x20;
/// transmit start address of descriptor 1 (4byte, C mode, 4 byte alignment)
const TSAD1: u16 = 0x24;
/// transmit normal priority descriptors start address (8bytes, C+ mode, 256 byte-align)
const TNPDS: u16 = 0x20;
/// transmit start address of descriptor 2 (4byte, C mode, 4 byte alignment)
const TSAD2: u16 = 0x28;
/// transmit start address of descriptor 3 (4byte, C mode, 4 byte alignment)
const TSAD3: u16 = 0x2c;
/// command register (1byte)
const CR: u16 = 0x37;
/// interrupt mask register (2byte)
const IMR: u16 = 0x3c;
/// interrupt status register (2byte)
const ISR: u16 = 0x3e;
/// transmit config register (4byte)
const TCR: u16 = 0x40;
/// receive config register (4byte)
const RCR: u16 = 0x44;
// command register for 93C46 (93C56) (1byte)
const CR9346: u16 = 0x50;
/// config register 0 (1byte)
const CONFIG0: u16 = 0x51;
/// config register 1 (1byte)
const CONFIG1: u16 = 0x52;
/// media status register (1byte)
const MSR: u16 = 0x58;
/// recieve buffer start address (C mode, 4 byte alignment)
const RBSTART: u16 = 0x30;
/// basic mode control register (2byte)
const BMCR: u16 = 0x62;
/// basic mode status register (2byte)
const BMSR: u16 = 0x64;

/// Reset, set to 1 to invoke S/W reset, held to 1 while resetting
const CR_RST: u8 = 0x10;
/// Reciever Enable, enables receiving
const CR_RE: u8 = 0x08;
/// Transmitter Enable, enables transmitting
const CR_TE: u8 = 0x04;
/// Rx buffer is empty
const CR_BUFE: u8 = 0x01;

// determine the operating mode
const CR9346_EEM1: u8 = 0x80;
/// 00 = Normal, 01 = Auto-load, 10 = Programming, 11 = Config, Register write enabled
const CR9346_EEM0: u8 = 0x40;
/// status of EESK
const CR9346_EESK: u8 = 0x4;
/// status of EEDI
const CR9346_EEDI: u8 = 0x2;
/// status of EEDO
const CR9346_EEDO: u8 = 0x1;

/// leds status
const CONFIG1_LEDS: u8 = 0xC0;
/// is the driver loaded ?
const CONFIG1_DVRLOAD: u8 = 0x20;
/// lanwake mode
const CONFIG1_LWACT: u8 = 0x10;
/// Memory mapping enabled ?
const CONFIG1_MEMMAP: u8 = 0x8;
/// IO map enabled ?
const CONFIG1_IOMAP: u8 = 0x4;
/// enable the virtal product data
const CONFIG1_VPD: u8 = 0x2;
/// Power Managment Enable
const CONFIG1_PMEN: u8 = 0x1;

// Media Status Register
const MSR_TXFCE: u8 = 0x80; // Tx Flow Control enabled
const MSR_RXFCE: u8 = 0x40; // Rx Flow Control enabled
const MSR_AS: u8 = 0x10; // Auxilary status
const MSR_SPEED: u8 = 0x8; // set if currently talking on 10mbps network, clear if 100mbps
const MSR_LINKB: u8 = 0x4; // Link Bad ?
const MSR_TXPF: u8 = 0x2; // Transmit Pause flag
const MSR_RXPF: u8 = 0x1; // Recieve Pause flag

const RCR_ERTH3: u32 = 0x0800_0000; // early Rx Threshold 0
const RCR_ERTH2: u32 = 0x0400_0000; // early Rx Threshold 1
const RCR_ERTH1: u32 = 0x0200_0000; // early Rx Threshold 2
const RCR_ERTH0: u32 = 0x0100_0000; // early Rx Threshold 3
const RCR_MRINT: u32 = 0x20000; // Multiple Early interrupt, (enable to make interrupts happen early, yuk)
const RCR_RER8: u32 = 0x10000; // Receive Error Packets larger than 8 bytes
const RCR_RXFTH2: u32 = 0x8000; // Rx Fifo threshold 0
const RCR_RXFTH1: u32 = 0x4000; // Rx Fifo threshold 1 (set to 110 and it will send to system when 1024bytes have been gathered)
const RCR_RXFTH0: u32 = 0x2000; // Rx Fifo threshold 2 (set all these to 1, and it wont FIFO till the full packet is ready)
const RCR_RBLEN1: u32 = 0x1000; // Rx Buffer length 0
const RCR_RBLEN0: u32 = 0x800; // Rx Buffer length 1 (C mode, 11 = 64kb, 10 = 32k, 01 = 16k, 00 = 8k)
const RCR_MXDMA2: u32 = 0x400; // Max DMA burst size 0
const RCR_MXDMA1: u32 = 0x200; // Max DMA burst size 1
const RCR_MXDMA0: u32 = 0x100; // Max DMA burst size 2
const RCR_WRAP: u32 = 0x80; // (void if buffer size = 64k, C mode, wrap to beginning of Rx buffer if we hit the end)
const RCR_EEPROMSEL: u32 = 0x40; // EEPROM type (0 = 9346, 1 = 9356)
const RCR_AER: u32 = 0x20; // Accept Error Packets (do we accept bad packets ?)
const RCR_AR: u32 = 0x10; // Accept runt packets (accept packets that are too small ?)
const RCR_AB: u32 = 0x08; // Accept Broadcast packets (accept broadcasts ?)
const RCR_AM: u32 = 0x04; // Accept multicast ?
const RCR_APM: u32 = 0x02; // Accept Physical matches (accept packets sent to our mac ?)
const RCR_AAP: u32 = 0x01; // Accept packets with a physical address ?

const TCR_HWVERID: u32 = 0x7CC0_0000; // mask for hw version ID's
const TCR_HWOFFSET: u32 = 22;
const TCR_IFG: u32 = 0x0300_0000; // interframe gap time
const TCR_LBK1: u32 = 0x40000; // loopback test
const TCR_LBK0: u32 = 0x20000; // loopback test
const TCR_CRC: u32 = 0x10000; // append CRC (card adds CRC if 1)
const TCR_MXDMA2: u32 = 0x400; // max dma burst
const TCR_MXDMA1: u32 = 0x200; // max dma burst
const TCR_MXDMA0: u32 = 0x100; // max dma burst
const TCR_TXRR: u32 = 0xF0; // Tx retry count, 0 = 16 else retries TXRR * 16 + 16 times
const TCR_CLRABT: u32 = 0x01; // Clear abort, attempt retransmit (when in abort state)

// Basic mode control register
const BMCR_RESET: u16 = 0x8000; // set the status and control of PHY to default
const BMCR_SPD100: u16 = (1 << 13); // 100 MBit
const BMCR_SPD1000: u16 = (1 << 6); // 1000 MBit
const BMCR_ANE: u16 = 0x1000; // enable N-way autonegotiation (ignore above if set)
const BMCR_RAN: u16 = 0x400; // restart auto-negotiation
const BMCR_DUPLEX: u16 = 0x200; // Duplex mode, generally a value of 1 means full-duplex

// Interrupt Status/Mask Register
// Bits in IMR enable/disable interrupts for specific events
// Bits in ISR indicate the status of the card
const ISR_SERR: u16 = 0x8000; // System error interrupt
const ISR_TUN: u16 = 0x4000; // time out interrupt
const ISR_SWINT: u16 = 0x100; // Software interrupt
const ISR_TDU: u16 = 0x80; // Tx Descriptor unavailable
const ISR_FIFOOVW: u16 = 0x40; // Rx Fifo overflow
const ISR_PUN: u16 = 0x20; // Packet underrun/link change
const ISR_RXOVW: u16 = 0x10; // Rx overflow/Rx Descriptor unavailable
const ISR_TER: u16 = 0x08; // Tx Error
const ISR_TOK: u16 = 0x04; // Tx OK
const ISR_RER: u16 = 0x02; // Rx Error
const ISR_ROK: u16 = 0x01; // Rx OK
const R39_INTERRUPT_MASK: u16 = 0x7f;

// Transmit Status of Descriptor0-3 (C mode only)
const TSD_CRS: u32 = (1 << 31); // carrier sense lost (during packet transmission)
const TSD_TABT: u32 = (1 << 30); // transmission abort
const TSD_OWC: u32 = (1 << 29); // out of window collision
const TSD_CDH: u32 = (1 << 28); // CD Heart beat (Cleared in 100Mb mode)
const TSD_NCC: u32 = 0x0F00_0000; // Number of collisions counted (during transmission)
const TSD_EARTH: u32 = 0x003F_0000; // threshold to begin transmission (0 = 8bytes, 1->2^6 = * 32bytes)
const TSD_TOK: u32 = (1 << 15); // Transmission OK, successful
const TSD_TUN: u32 = (1 << 14); // Transmission FIFO underrun
const TSD_OWN: u32 = (1 << 13); // Tx DMA operation finished (driver must set to 0 when TBC is written)
const TSD_SIZE: u32 = 0x1fff; // Descriptor size, the total size in bytes of data to send (max 1792)

/// To set the RTL8139 to accept only the Transmit OK (TOK) and Receive OK (ROK)
/// interrupts, we would have the TOK and ROK bits of the IMR high and leave the
/// rest low. That way when a TOK or ROK IRQ happens, it actually will go through
/// and fire up an IRQ.
const INT_MASK: u16 = (ISR_ROK | ISR_TOK | ISR_RXOVW | ISR_TER | ISR_RER);

/// Beside Receive OK (ROK) interrupt, this mask enable all other interrupts
const INT_MASK_NO_ROK: u16 = (ISR_TOK | ISR_RXOVW | ISR_TER | ISR_RER);

const NO_TX_BUFFERS: usize = 4;

static TX_ID: AtomicUsize = AtomicUsize::new(0);
static mut TX_IN_USE: [bool; NO_TX_BUFFERS] = [false; NO_TX_BUFFERS];
static mut IOBASE: u16 = 0;

static POOLING: AtomicBool = AtomicBool::new(false);

fn is_pooling() -> bool {
	POOLING.load(Ordering::SeqCst)
}

extern "C" fn rtl8139_thread(arg: usize) {
	let adapter;

	debug!("Hello from network thread!");

	unsafe {
		adapter = *(arg as *const pci::PciAdapter);
		IOBASE = adapter.base_addresses[0].try_into().unwrap();
		info!(
			"Found RTL8139 at iobase 0x{:x} (irq {})",
			IOBASE, adapter.irq
		);
	}

	::arch::irq::disable();

	let neighbor_cache = NeighborCache::new(BTreeMap::new());
	let ethernet_addr;
	unsafe {
		ethernet_addr = EthernetAddress([
			inb(IOBASE + IDR0 + 0),
			inb(IOBASE + IDR0 + 1),
			inb(IOBASE + IDR0 + 2),
			inb(IOBASE + IDR0 + 3),
			inb(IOBASE + IDR0 + 4),
			inb(IOBASE + IDR0 + 5),
		]);
	}
	let ip_addrs = [IpCidr::new(IpAddress::v4(10, 0, 2, 5), 24)];
	//let ip_addrs = [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)];
	let default_gw = Ipv4Address::new(10, 0, 2, 2);
	let mut routes_storage = [None; 1];
	let mut routes = Routes::new(&mut routes_storage[..]);
	routes.add_default_ipv4_route(default_gw).unwrap();

	info!("MAC address {}", ethernet_addr);

	unsafe {
		if inl(IOBASE + TCR) == 0x00FF_FFFFu32 {
			info!("Unable to initialize RTL 8192");
			return;
		}

		// Software reset
		outb(IOBASE + CR, CR_RST);

		// The RST bit must be checked to make sure that the chip has finished the reset.
		// If the RST bit is high (1), then the reset is still in operation.
		::arch::kernel::processor::udelay(10000);
		let mut tmp: u16 = 10000;
		while (inb(IOBASE + CR) & CR_RST) == CR_RST && tmp > 0 {
			tmp -= 1;
		}

		if tmp == 0 {
			info!("RTL8139 reset failed");
			return;
		}

		// Enable Receive and Transmitter
		outb(IOBASE + CR, CR_TE | CR_RE); // Sets the RE and TE bits high

		// lock config register
		outb(IOBASE + CR9346, CR9346_EEM1 | CR9346_EEM0);

		// clear all of CONFIG1
		outb(IOBASE + CONFIG1, 0);

		// disable driver loaded and lanwake bits, turn driver loaded bit back on
		outb(
			IOBASE + CONFIG1,
			(inb(IOBASE + CONFIG1) & !(CONFIG1_DVRLOAD | CONFIG1_LWACT)) | CONFIG1_DVRLOAD,
		);

		// unlock config register
		outb(IOBASE + CR9346, 0);

		/*
		 * configure receive buffer
		 * AB - Accept Broadcast: Accept broadcast packets sent to mac ff:ff:ff:ff:ff:ff
		 * AM - Accept Multicast: Accept multicast packets.
		 * APM - Accept Physical Match: Accept packets send to NIC's MAC address.
		 * AAP - Accept All Packets. Accept all packets (run in promiscuous mode).
		 */
		outl(
			IOBASE + RCR,
			RCR_MXDMA2 | RCR_MXDMA1 | RCR_MXDMA0 | RCR_AB | RCR_AM | RCR_APM | RCR_AAP,
		); // The WRAP bit isn't set!

		// set the transmit config register to
		// be the normal interframe gap time
		// set DMA max burst to 64bytes
		outl(IOBASE + TCR, TCR_IFG | TCR_MXDMA0 | TCR_MXDMA1 | TCR_MXDMA2);
	}

	let rxbuffer = ::mm::allocate_iomem(RX_BUF_LEN);
	let txbuffer = ::mm::allocate_iomem(NO_TX_BUFFERS * TX_BUF_LEN);
	if txbuffer == 0 || rxbuffer == 0 {
		error!("Unable to allocate buffers for RTL8139");
		return;
	}
	debug!(
		"Allocate TxBuffer at 0x{:x} and RxBuffer at 0x{:x}",
		txbuffer, rxbuffer
	);
	let device = RTL8139::new(rxbuffer, txbuffer);

	unsafe {
		// register the receive buffer
		outl(IOBASE + RBSTART, virt_to_phys(rxbuffer).try_into().unwrap());

		// set each of the transmitter start address descriptors
		for i in 0..NO_TX_BUFFERS {
			outl(
				IOBASE + TSAD0,
				virt_to_phys(txbuffer + i * TX_BUF_LEN).try_into().unwrap(),
			);
		}

		// Enable all known interrupts by setting the interrupt mask.
		outw(IOBASE + IMR, INT_MASK);

		outw(IOBASE + BMCR, BMCR_ANE);
		let speed;
		let tmp = inw(IOBASE + BMCR);
		if tmp & BMCR_SPD1000 == BMCR_SPD1000 {
			speed = 1000;
		} else if tmp & BMCR_SPD100 == BMCR_SPD100 {
			speed = 100;
		} else {
			speed = 10;
		}

		// Enable Receive and Transmitter
		outb(IOBASE + CR, CR_TE | CR_RE); // Sets the RE and TE bits high

		info!(
			"RTL8139: CR = 0x{:x}, ISR = 0x{:x}, speed = {} mbps",
			inb(IOBASE + CR),
			inw(IOBASE + ISR),
			speed
		);
	}

	let mut iface = EthernetInterfaceBuilder::new(device)
		.ethernet_addr(ethernet_addr)
		.neighbor_cache(neighbor_cache)
		.ip_addrs(ip_addrs)
		.routes(routes)
		.finalize();

	// Install interrupt handler for RTL8139
	debug!("Install interrupt handler for RTL8139 at {}", adapter.irq);
	irq_install_handler(adapter.irq.into(), rtl8139_irqhandler as usize);

	::arch::irq::enable();

	networkd(&mut iface, is_pooling);
}

unsafe fn tx_handler() {
	for i in 0..TX_IN_USE.len() {
		if TX_IN_USE[i] {
			let txstatus = inl(IOBASE + TSD0 + i as u16 * 4);

			if (txstatus & (TSD_TABT | TSD_OWC)) > 0 {
				error!("RTL8139: major error\n");
				continue;
			}

			if (txstatus & TSD_TUN) == TSD_TUN {
				error!("RTL8139: transmit underrun\n");
			}

			if (txstatus & TSD_TOK) == TSD_TOK {
				TX_IN_USE[i] = false;
			}
		}
	}
}

extern "x86-interrupt" fn rtl8139_irqhandler(_stack_frame: &mut ExceptionStackFrame) {
	debug!("Receive network interrupt from RTL8139");

	unsafe {
		let mut isr_contents = inw(IOBASE + ISR);
		while isr_contents != 0 {
			if (isr_contents & ISR_ROK) == ISR_ROK && !is_pooling() {
				info!("Wakeup network thread!");
				// disable interrupts from the NIC
				outw(IOBASE + IMR, INT_MASK_NO_ROK);
				// switch to polling mode
				POOLING.store(true, Ordering::SeqCst);
				NET_SEM.release();
			}

			if (isr_contents & ISR_TOK) == ISR_TOK {
				tx_handler();
			}

			if (isr_contents & ISR_RER) == ISR_RER {
				error!("RTL88139: RX error detected!\n");
			}

			if (isr_contents & ISR_TER) == ISR_TER {
				error!("RTL88139r: TX error detected!\n");
			}

			if (isr_contents & ISR_RXOVW) == ISR_RXOVW {
				error!("RTL88139: RX overflow detected!\n");
			}

			outw(
				IOBASE + ISR,
				isr_contents & (ISR_RXOVW | ISR_TER | ISR_RER | ISR_TOK | ISR_ROK),
			);

			isr_contents = inw(IOBASE + ISR);
		}
	}

	apic::eoi();
	core_scheduler().scheduler();
}

/// A network device for uhyve.
pub struct RTL8139 {
	rxbuffer: usize,
	txbuffer: usize,
	rxpos: usize,
}

impl RTL8139 {
	pub fn new(rxbuffer: usize, txbuffer: usize) -> Self {
		RTL8139 {
			rxbuffer,
			txbuffer,
			rxpos: 0,
		}
	}
}

impl<'a> Device<'a> for RTL8139 {
	type RxToken = RxToken;
	type TxToken = TxToken;

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = 1500;
		cap
	}

	fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
		let cmd = unsafe { inb(IOBASE + CR as u16) };

		if (cmd & CR_BUFE) != CR_BUFE {
			let header: u16 = unsafe { *((self.rxbuffer + self.rxpos) as *const u16) };
			self.rxpos = (self.rxpos + mem::size_of::<u16>()) % RX_BUF_LEN;

			if header & ISR_ROK == ISR_ROK {
				let length: u16 = unsafe { *((self.rxbuffer + self.rxpos) as *const u16) } - 4; // copy packet (but not the CRC)
				self.rxpos = (self.rxpos + mem::size_of::<u16>()) % RX_BUF_LEN;

				debug!("resize message to {} bytes", length);

				let tx = TxToken::new(self.txbuffer);
				let rx = RxToken::new(self.rxbuffer, length as usize);

				// check also output buffers
				unsafe {
					tx_handler();
				}

				return Some((rx, tx));
			} else {
				error!(
					"RTL8192: invalid header 0x{:x}, rx_pos {}\n",
					header, self.rxpos
				);
			}
		}

		POOLING.store(false, Ordering::SeqCst);
		// enable all known interrupts
		unsafe {
			outw(IOBASE + IMR, INT_MASK);
		}

		None
	}

	fn transmit(&'a mut self) -> Option<Self::TxToken> {
		Some(TxToken::new(self.txbuffer))
	}
}

#[doc(hidden)]
pub struct RxToken {
	rxbuffer: usize,
	len: usize,
}

impl RxToken {
	pub fn new(addr: usize, len: usize) -> RxToken {
		RxToken {
			rxbuffer: addr,
			len: len,
		}
	}
}

impl phy::RxToken for RxToken {
	fn consume<R, F>(self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&[u8]) -> smoltcp::Result<R>,
	{
		let buffer = unsafe { core::slice::from_raw_parts(self.rxbuffer as *mut u8, RX_BUF_LEN) };
		let (first, _) = buffer.split_at(self.len);
		f(first)
	}
}

#[doc(hidden)]
pub struct TxToken {
	txbuffer: usize,
}

impl TxToken {
	pub fn new(txbuffer: usize) -> Self {
		TxToken { txbuffer: txbuffer }
	}
}

impl phy::TxToken for TxToken {
	fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		let id = TX_ID.fetch_add(1, Ordering::SeqCst) % NO_TX_BUFFERS;

		unsafe {
			if TX_IN_USE[id] {
				Err(smoltcp::Error::Dropped)
			} else if len > TX_BUF_LEN {
				Err(smoltcp::Error::Exhausted)
			} else if (inb(IOBASE + MSR) & MSR_LINKB) == MSR_LINKB {
				Err(smoltcp::Error::Illegal)
			} else {
				let mut buffer = core::slice::from_raw_parts_mut(
					(self.txbuffer + id * TX_BUF_LEN) as *mut u8,
					len,
				);
				let result = f(&mut buffer);
				if result.is_ok() {
					TX_IN_USE[id] = true;

					// send the packet
					outl(
						IOBASE + TSD0 as u16 + (4 * id as u16),
						len.try_into().unwrap(),
					); //|0x3A0000);
				}
				result
			}
		}
	}
}

struct Boards {
	pub vendor_name: &'static str,
	pub device_name: &'static str,
	pub vendor_id: u16,
	pub device_id: u16,
}

impl Boards {
	pub const fn new(
		vendor_name: &'static str,
		device_name: &'static str,
		vendor_id: u16,
		device_id: u16,
	) -> Self {
		Boards {
			vendor_name: vendor_name,
			device_name: device_name,
			vendor_id: vendor_id,
			device_id: device_id,
		}
	}
}

static BOARDS: [Boards; 7] = [
	Boards::new("RealTek", "RealTek RTL8139", 0x10ec, 0x8139),
	Boards::new("RealTek", "RealTek RTL8129 Fast Ethernet", 0x10ec, 0x8129),
	Boards::new("RealTek", "RealTek RTL8139B PCI", 0x10ec, 0x8138),
	Boards::new(
		"SMC",
		"SMC1211TX EZCard 10/100 (RealTek RTL8139)",
		0x1113,
		0x1211,
	),
	Boards::new("D-Link", "D-Link DFE-538TX (RTL8139)", 0x1186, 0x1300),
	Boards::new("LevelOne", "LevelOne FPC-0106Tx (RTL8139)", 0x018a, 0x0106),
	Boards::new("Compaq", "Compaq HNE-300 (RTL8139c)", 0x021b, 0x8139),
];

fn find_adapter() -> Result<pci::PciAdapter, ()> {
	for i in 0..BOARDS.len() {
		match pci::get_adapter(BOARDS[i].vendor_id, BOARDS[i].device_id) {
			Some(adapter) => {
				return Ok(adapter);
			}
			_ => {}
		}
	}

	Err(())
}
pub fn init() -> Result<(), ()> {
	let adapter = find_adapter()?;
	adapter.make_bus_master();

	let core_scheduler = core_scheduler();
	unsafe {
		NETWORK_TASK_ID = core_scheduler.spawn(
			rtl8139_thread,
			&adapter as *const _ as usize,
			scheduler::task::HIGH_PRIO,
			Some(crate::arch::mm::virtualmem::task_heap_start()),
		);
	}

	mem::forget(adapter);
	core_scheduler.scheduler();

	Ok(())
}
