// The driver based on the online manual http://www.lowlevel.eu/wiki/RTL8139

#![allow(dead_code)]

use alloc::boxed::Box;
use core::mem;

use pci_types::{Bar, CommandRegister, InterruptLine, MAX_BARS};
use x86::io::*;

use crate::arch::kernel::core_local::increment_irq_counter;
use crate::arch::kernel::interrupts::*;
use crate::arch::mm::paging::virt_to_phys;
use crate::arch::mm::VirtAddr;
use crate::arch::pci::PciConfigRegion;
use crate::drivers::error::DriverError;
use crate::drivers::net::{network_irqhandler, NetworkDriver};
use crate::drivers::pci::PciDevice;
use crate::executor::device::{RxToken, TxToken};

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
/// current address of packet read (2byte, C mode, initial value 0xFFF0)
const CAPR: u16 = 0x38;
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
/// receive buffer start address (C mode, 4 byte alignment)
const RBSTART: u16 = 0x30;
/// basic mode control register (2byte)
const BMCR: u16 = 0x62;
/// basic mode status register (2byte)
const BMSR: u16 = 0x64;

/// Reset, set to 1 to invoke S/W reset, held to 1 while resetting
const CR_RST: u8 = 0x10;
/// Receiver Enable, enables receiving
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
/// enable the virtual product data
const CONFIG1_VPD: u8 = 0x2;
/// Power Management Enable
const CONFIG1_PMEN: u8 = 0x1;

// Media Status Register
const MSR_TXFCE: u8 = 0x80; // Tx Flow Control enabled
const MSR_RXFCE: u8 = 0x40; // Rx Flow Control enabled
const MSR_AS: u8 = 0x10; // Auxiliary status
const MSR_SPEED: u8 = 0x8; // set if currently talking on 10mbps network, clear if 100mbps
const MSR_LINKB: u8 = 0x4; // Link Bad ?
const MSR_TXPF: u8 = 0x2; // Transmit Pause flag
const MSR_RXPF: u8 = 0x1; // Receive Pause flag

const RCR_ERTH3: u32 = 0x0800_0000; // early Rx Threshold 0
const RCR_ERTH2: u32 = 0x0400_0000; // early Rx Threshold 1
const RCR_ERTH1: u32 = 0x0200_0000; // early Rx Threshold 2
const RCR_ERTH0: u32 = 0x0100_0000; // early Rx Threshold 3
const RCR_MRINT: u32 = 0x20000; // Multiple Early interrupt, (enable to make interrupts happen early, yuk)
const RCR_RER8: u32 = 0x10000; // Receive Error Packets larger than 8 bytes
const RCR_RXFTH2: u32 = 0x8000; // Rx Fifo threshold 0
const RCR_RXFTH1: u32 = 0x4000; // Rx Fifo threshold 1 (set to 110 and it will send to system when 1024bytes have been gathered)
const RCR_RXFTH0: u32 = 0x2000; // Rx Fifo threshold 2 (set all these to 1, and it won't FIFO till the full packet is ready)
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
const BMCR_SPD100: u16 = 1 << 13; // 100 MBit
const BMCR_SPD1000: u16 = 1 << 6; // 1000 MBit
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
const TSD_CRS: u32 = 1 << 31; // carrier sense lost (during packet transmission)
const TSD_TABT: u32 = 1 << 30; // transmission abort
const TSD_OWC: u32 = 1 << 29; // out of window collision
const TSD_CDH: u32 = 1 << 28; // CD Heart beat (Cleared in 100Mb mode)
const TSD_NCC: u32 = 0x0F00_0000; // Number of collisions counted (during transmission)
const TSD_EARTH: u32 = 0x003F_0000; // threshold to begin transmission (0 = 8bytes, 1->2^6 = * 32bytes)
const TSD_TOK: u32 = 1 << 15; // Transmission OK, successful
const TSD_TUN: u32 = 1 << 14; // Transmission FIFO underrun
const TSD_OWN: u32 = 1 << 13; // Tx DMA operation finished (driver must set to 0 when TBC is written)
const TSD_SIZE: u32 = 0x1fff; // Descriptor size, the total size in bytes of data to send (max 1792)

/// To set the RTL8139 to accept only the Transmit OK (TOK) and Receive OK (ROK)
/// interrupts, we would have the TOK and ROK bits of the IMR high and leave the
/// rest low. That way when a TOK or ROK IRQ happens, it actually will go through
/// and fire up an IRQ.
const INT_MASK: u16 = ISR_ROK | ISR_TOK | ISR_RXOVW | ISR_TER | ISR_RER;

/// Beside Receive OK (ROK) interrupt, this mask enable all other interrupts
const INT_MASK_NO_ROK: u16 = ISR_TOK | ISR_RXOVW | ISR_TER | ISR_RER;

const NO_TX_BUFFERS: usize = 4;

#[derive(Debug)]
pub enum RTL8139Error {
	InitFailed,
	ResetFailed,
	Unknown,
}

/// RealTek RTL8139 network driver struct.
///
/// Struct allows to control device queues as also
/// the device itself.
pub(crate) struct RTL8139Driver {
	iobase: u16,
	mtu: u16,
	irq: InterruptLine,
	mac: [u8; 6],
	tx_in_use: [bool; NO_TX_BUFFERS],
	tx_counter: usize,
	rxbuffer: Box<[u8]>,
	rxpos: usize,
	txbuffer: Box<[u8]>,
}

impl NetworkDriver for RTL8139Driver {
	/// Returns the MAC address of the network interface
	fn get_mac_address(&self) -> [u8; 6] {
		self.mac
	}

	/// Returns the current MTU of the device.
	fn get_mtu(&self) -> u16 {
		self.mtu
	}

	/// Send packet with the size `len`
	fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		let id = self.tx_counter % NO_TX_BUFFERS;

		if self.tx_in_use[id] || len > TX_BUF_LEN {
			panic!("Unable to get TX buffer");
		} else {
			self.tx_in_use[id] = true;
			self.tx_counter += 1;

			let buffer = &mut self.txbuffer[id * TX_BUF_LEN..][..len];
			let result = f(buffer);

			// send the packet
			unsafe {
				outl(
					self.iobase + TSD0 + (4 * id as u16),
					len.try_into().unwrap(),
				); //|0x3A0000);
			}

			result
		}
	}

	fn has_packet(&self) -> bool {
		let cmd = unsafe { inb(self.iobase + CR) };

		if (cmd & CR_BUFE) != CR_BUFE {
			let header = self.rx_peek_u16();

			if header & ISR_ROK == ISR_ROK {
				return true;
			}
		}

		false
	}

	/// Get buffer with the received packet
	fn receive_packet(&mut self) -> Option<(RxToken, TxToken)> {
		let cmd = unsafe { inb(self.iobase + CR) };

		if (cmd & CR_BUFE) != CR_BUFE {
			let header = self.rx_peek_u16();
			self.advance_rxpos(mem::size_of::<u16>());

			if header & ISR_ROK == ISR_ROK {
				let length = self.rx_peek_u16() - 4; // copy packet (but not the CRC)
				let pos = (self.rxpos + mem::size_of::<u16>()) % RX_BUF_LEN;

				// do we reach the end of the receive buffers?
				// in this case, we conact the two slices to one vec
				let vec_data = if pos + length as usize > RX_BUF_LEN {
					let first = &self.rxbuffer[pos..RX_BUF_LEN];
					let second = &self.rxbuffer[..length as usize - first.len()];
					[first, second].concat()
				} else {
					(self.rxbuffer[pos..][..length.into()]).to_vec()
				};

				self.consume_current_buffer();

				Some((RxToken::new(vec_data), TxToken::new()))
			} else {
				warn!(
					"RTL8192: invalid header {:#x}, rx_pos {}\n",
					header, self.rxpos
				);

				None
			}
		} else {
			None
		}
	}

	fn set_polling_mode(&mut self, value: bool) {
		if value {
			unsafe {
				outw(self.iobase + IMR, INT_MASK_NO_ROK);
			}
		} else {
			// Enable all known interrupts by setting the interrupt mask.
			unsafe {
				outw(self.iobase + IMR, INT_MASK);
			}
		}
	}

	fn handle_interrupt(&mut self) -> bool {
		increment_irq_counter(32 + self.irq);

		let isr_contents = unsafe { inw(self.iobase + ISR) };

		if (isr_contents & ISR_TOK) == ISR_TOK {
			self.tx_handler();
		}

		if (isr_contents & ISR_RER) == ISR_RER {
			error!("RTL88139: RX error detected!\n");
		}

		if (isr_contents & ISR_TER) == ISR_TER {
			trace!("RTL88139r: TX error detected!\n");
		}

		if (isr_contents & ISR_RXOVW) == ISR_RXOVW {
			trace!("RTL88139: RX overflow detected!\n");
		}

		let ret = (isr_contents & ISR_ROK) == ISR_ROK;

		unsafe {
			outw(
				self.iobase + ISR,
				isr_contents & (ISR_RXOVW | ISR_TER | ISR_RER | ISR_TOK | ISR_ROK),
			);
		}

		ret
	}
}

impl RTL8139Driver {
	// Tells driver, that buffer is consumed and can be deallocated
	fn consume_current_buffer(&mut self) {
		let length = self.rx_peek_u16();
		self.advance_rxpos(usize::from(length) + mem::size_of::<u16>());

		// packets are dword aligned
		self.rxpos = ((self.rxpos + 3) & !0x3) % RX_BUF_LEN;
		if self.rxpos >= 0x10 {
			unsafe {
				outw(self.iobase + CAPR, (self.rxpos - 0x10).try_into().unwrap());
			}
		} else {
			unsafe {
				outw(
					self.iobase + CAPR,
					(RX_BUF_LEN - (0x10 - self.rxpos)).try_into().unwrap(),
				);
			}
		}
	}

	fn rx_peek_u16(&self) -> u16 {
		u16::from_ne_bytes(
			self.rxbuffer[self.rxpos..][..mem::size_of::<u16>()]
				.try_into()
				.unwrap(),
		)
	}

	fn advance_rxpos(&mut self, count: usize) {
		self.rxpos += count;
		self.rxpos %= RX_BUF_LEN;
	}

	fn tx_handler(&mut self) {
		for i in 0..self.tx_in_use.len() {
			if self.tx_in_use[i] {
				let txstatus = unsafe { inl(self.iobase + TSD0 + i as u16 * 4) };

				if (txstatus & (TSD_TABT | TSD_OWC)) > 0 {
					error!("RTL8139: major error");
					continue;
				}

				if (txstatus & TSD_TUN) == TSD_TUN {
					error!("RTL8139: transmit underrun");
				}

				if (txstatus & TSD_TOK) == TSD_TOK {
					self.tx_in_use[i] = false;
				}
			}
		}
	}
}

impl Drop for RTL8139Driver {
	fn drop(&mut self) {
		debug!("Dropping RTL8129Driver!");

		// Software reset
		unsafe {
			outb(self.iobase + CR, CR_RST);
		}
	}
}

pub(crate) fn init_device(
	device: &PciDevice<PciConfigRegion>,
) -> Result<RTL8139Driver, DriverError> {
	let irq = device.get_irq().unwrap();
	let mut iobase: Option<u32> = None;

	for i in 0..MAX_BARS {
		if let Some(Bar::Io { port }) = device.get_bar(i.try_into().unwrap()) {
			iobase = Some(port)
		}
	}

	let iobase: u16 = iobase
		.ok_or(DriverError::InitRTL8139DevFail(RTL8139Error::Unknown))?
		.try_into()
		.unwrap();

	debug!("Found RTL8139 at iobase {:#x} (irq {})", iobase, irq);

	device.set_command(CommandRegister::BUS_MASTER_ENABLE);

	let mac: [u8; 6] = unsafe {
		[
			inb(iobase + IDR0),
			inb(iobase + IDR0 + 1),
			inb(iobase + IDR0 + 2),
			inb(iobase + IDR0 + 3),
			inb(iobase + IDR0 + 4),
			inb(iobase + IDR0 + 5),
		]
	};

	debug!(
		"MAC address {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
		mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
	);

	unsafe {
		if inl(iobase + TCR) == 0x00FF_FFFFu32 {
			error!("Unable to initialize RTL8192");
			return Err(DriverError::InitRTL8139DevFail(RTL8139Error::InitFailed));
		}

		// Software reset
		outb(iobase + CR, CR_RST);

		// The RST bit must be checked to make sure that the chip has finished the reset.
		// If the RST bit is high (1), then the reset is still in operation.
		crate::arch::kernel::processor::udelay(10000);
		let mut tmp: u16 = 10000;
		while (inb(iobase + CR) & CR_RST) == CR_RST && tmp > 0 {
			tmp -= 1;
		}

		if tmp == 0 {
			error!("RTL8139 reset failed");
			return Err(DriverError::InitRTL8139DevFail(RTL8139Error::ResetFailed));
		}

		// Enable Receive and Transmitter
		outb(iobase + CR, CR_TE | CR_RE); // Sets the RE and TE bits high

		// lock config register
		outb(iobase + CR9346, CR9346_EEM1 | CR9346_EEM0);

		// clear all of CONFIG1
		outb(iobase + CONFIG1, 0);

		// disable driver loaded and lanwake bits, turn driver loaded bit back on
		outb(
			iobase + CONFIG1,
			(inb(iobase + CONFIG1) & !(CONFIG1_DVRLOAD | CONFIG1_LWACT)) | CONFIG1_DVRLOAD,
		);

		// unlock config register
		outb(iobase + CR9346, 0);

		// configure receive buffer
		// AB - Accept Broadcast: Accept broadcast packets sent to mac ff:ff:ff:ff:ff:ff
		// AM - Accept Multicast: Accept multicast packets.
		// APM - Accept Physical Match: Accept packets send to NIC's MAC address.
		// AAP - Accept All Packets. Accept all packets (run in promiscuous mode).
		outl(
			iobase + RCR,
			RCR_MXDMA2 | RCR_MXDMA1 | RCR_MXDMA0 | RCR_AB | RCR_AM | RCR_APM | RCR_AAP,
		); // The WRAP bit isn't set!

		// set the transmit config register to
		// be the normal interframe gap time
		// set DMA max burst to 64bytes
		outl(iobase + TCR, TCR_IFG | TCR_MXDMA0 | TCR_MXDMA1 | TCR_MXDMA2);
	}

	let rxbuffer = vec![0; RX_BUF_LEN].into_boxed_slice();
	let txbuffer = vec![0; NO_TX_BUFFERS * TX_BUF_LEN].into_boxed_slice();

	debug!(
		"Allocate TxBuffer at {:p} and RxBuffer at {:p}",
		txbuffer, rxbuffer
	);

	let phys_addr = |p| {
		virt_to_phys(VirtAddr::from_usize(p as _))
			.as_u64()
			.try_into()
			.unwrap()
	};

	unsafe {
		// register the receive buffer
		outl(iobase + RBSTART, phys_addr(rxbuffer.as_ptr()));

		// set each of the transmitter start address descriptors
		outl(iobase + TSAD0, phys_addr(txbuffer[..TX_BUF_LEN].as_ptr()));
		outl(
			iobase + TSAD1,
			phys_addr(txbuffer[TX_BUF_LEN..][..TX_BUF_LEN].as_ptr()),
		);
		outl(
			iobase + TSAD2,
			phys_addr(txbuffer[2 * TX_BUF_LEN..][..TX_BUF_LEN].as_ptr()),
		);
		outl(
			iobase + TSAD3,
			phys_addr(txbuffer[3 * TX_BUF_LEN..][..TX_BUF_LEN].as_ptr()),
		);

		// Enable all known interrupts by setting the interrupt mask.
		outw(iobase + IMR, INT_MASK);

		outw(iobase + BMCR, BMCR_ANE);
		let speed;
		let tmp = inw(iobase + BMCR);
		if tmp & BMCR_SPD1000 == BMCR_SPD1000 {
			speed = 1000;
		} else if tmp & BMCR_SPD100 == BMCR_SPD100 {
			speed = 100;
		} else {
			speed = 10;
		}

		// Enable Receive and Transmitter
		outb(iobase + CR, CR_TE | CR_RE); // Sets the RE and TE bits high

		info!(
			"RTL8139: CR = {:#x}, ISR = {:#x}, speed = {} mbps",
			inb(iobase + CR),
			inw(iobase + ISR),
			speed
		);
	}

	// Install interrupt handler for RTL8139
	debug!("Install interrupt handler for RTL8139 at {}", irq);
	irq_install_handler(irq, network_irqhandler);
	add_irq_name(irq, "rtl8139_net");

	Ok(RTL8139Driver {
		iobase,
		mtu: 1514,
		irq,
		mac,
		tx_in_use: [false; NO_TX_BUFFERS],
		tx_counter: 0,
		rxbuffer,
		rxpos: 0,
		txbuffer,
	})
}
