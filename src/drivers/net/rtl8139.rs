// The driver based on the online manual http://www.lowlevel.eu/wiki/RTL8139

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::hint::spin_loop;
use core::mem::{self, ManuallyDrop};
use core::ptr::NonNull;

use endian_num::{le16, le32, le64};
use pci_types::{Bar, CommandRegister, InterruptLine, MAX_BARS};
use smoltcp::phy::DeviceCapabilities;
use thiserror::Error;
use volatile::access::{NoAccess, ReadOnly, ReadWrite};
use volatile::{VolatileFieldAccess, VolatilePtr, VolatileRef, map_field};

use crate::arch::kernel::interrupts::*;
use crate::arch::pci::PciConfigRegion;
use crate::drivers::Driver;
use crate::drivers::error::DriverError;
use crate::drivers::net::{NetworkDriver, mtu};
use crate::drivers::pci::PciDevice;
use crate::executor::network::wake_network_waker;
use crate::mm::device_alloc::DeviceAlloc;

/// Size of the receive buffer
const RX_BUF_LEN: usize = 8192;
/// Size of the send buffer
const TX_BUF_LEN: usize = 4096;

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
/// Status of EESK
const CR9346_EESK: u8 = 0x4;
/// Status of EEDI
const CR9346_EEDI: u8 = 0x2;
/// Status of EEDO
const CR9346_EEDO: u8 = 0x1;

/// Leds status
const CONFIG1_LEDS: u8 = 0xc0;
/// Is the driver loaded ?
const CONFIG1_DVRLOAD: u8 = 0x20;
/// Lanwake mode
const CONFIG1_LWACT: u8 = 0x10;
/// Memory mapping enabled ?
const CONFIG1_MEMMAP: u8 = 0x8;
/// IO map enabled ?
const CONFIG1_IOMAP: u8 = 0x4;
/// Enable the virtual product data
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

const TCR_HWVERID: u32 = 0x7cc0_0000; // mask for hw version ID's
const TCR_HWOFFSET: u32 = 22;
const TCR_IFG: u32 = 0x0300_0000; // interframe gap time
const TCR_LBK1: u32 = 0x40000; // loopback test
const TCR_LBK0: u32 = 0x20000; // loopback test
const TCR_CRC: u32 = 0x10000; // append CRC (card adds CRC if 1)
const TCR_MXDMA2: u32 = 0x400; // max dma burst
const TCR_MXDMA1: u32 = 0x200; // max dma burst
const TCR_MXDMA0: u32 = 0x100; // max dma burst
const TCR_TXRR: u32 = 0xf0; // Tx retry count, 0 = 16 else retries TXRR * 16 + 16 times
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
const TSD_NCC: u32 = 0x0f00_0000; // Number of collisions counted (during transmission)
const TSD_EARTH: u32 = 0x003f_0000; // threshold to begin transmission (0 = 8bytes, 1->2^6 = * 32bytes)
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

/// http://realtek.info/pdf/rtl8139d.pdf
/// See "5. Register Descriptions"
#[repr(C)]
#[derive(VolatileFieldAccess)]
struct Regs {
	/// ID register 0.
	#[access(ReadOnly)] // r/o because writes require 4-byte access
	idr0: u8,
	/// ID register 1.
	#[access(ReadOnly)]
	idr1: u8,
	/// ID register 2.
	#[access(ReadOnly)]
	idr2: u8,
	/// ID register 3.
	#[access(ReadOnly)]
	idr3: u8,
	/// ID register 4.
	#[access(ReadOnly)]
	idr4: u8,
	/// ID register 5.
	#[access(ReadOnly)]
	idr5: u8,
	#[access(NoAccess)]
	__reserved0: [u8; 2],
	/// Multicast registers.
	#[access(ReadWrite)]
	mar: [u8; 8], // r/o because writes require 4-byte access
	/// Transmit status of descriptor 0.
	#[access(ReadWrite)]
	tsd0: le32,
	/// Transmit status of descriptor 1.
	#[access(ReadWrite)]
	tsd1: le32,
	/// Transmit status of descriptor 2.
	#[access(ReadWrite)]
	tsd2: le32,
	/// Transmit status of descriptor 3.
	#[access(ReadWrite)]
	tsd3: le32,
	/// Transmit start address of descriptor 0.
	#[access(ReadWrite)]
	tsad0: le32,
	/// Transmit start address of descriptor 1.
	#[access(ReadWrite)]
	tsad1: le32,
	/// Transmit start address of descriptor 2.
	#[access(ReadWrite)]
	tsad2: le32,
	/// Transmit start address of descriptor 3.
	#[access(ReadWrite)]
	tsad3: le32,
	/// Receive buffer start address.
	#[access(ReadWrite)]
	rbstart: le32,
	/// Early receive byte count register.
	#[access(ReadOnly)]
	erbcr: le16,
	/// Early rx status register.
	#[access(ReadOnly)]
	ersr: u8,
	/// Command register.
	#[access(ReadWrite)]
	cr: u8,
	/// Current address of packet read.
	#[access(ReadWrite)]
	capr: le16,
	/// Current buffer address.
	///
	/// Reflects total received byte count in the rx-buffer.
	#[access(ReadOnly)]
	cbr: le16,
	/// Interrupt mask register.
	#[access(ReadWrite)]
	imr: le16,
	/// Interrupt status register.
	#[access(ReadWrite)]
	isr: le16,
	/// Transmit configuration register.
	#[access(ReadWrite)]
	tcr: le32,
	/// Receive configuration register.
	#[access(ReadWrite)]
	rcr: le32,
	/// Timer count register.
	#[access(ReadWrite)]
	tctr: le32,
	/// Missed packet counter.
	///
	/// Indicates number of packets discarded due to rx fifo overflow.
	#[access(ReadWrite)]
	mpc: le32,
	/// 93C46 command register
	#[access(ReadWrite)]
	cr_9346: u8,
	/// Configuration register 0.
	#[access(ReadWrite)]
	config0: u8,
	/// Configuration register 1.
	#[access(ReadWrite)]
	config1: u8,
	#[access(NoAccess)]
	__reserved1: u8,
	/// Timer interrupt register.
	#[access(ReadWrite)]
	timer_int: le32,
	/// Media status register.
	#[access(ReadWrite)]
	msr: u8,
	/// Configuration register 3.
	#[access(ReadWrite)]
	config3: u8,
	/// Configuration register 4.
	#[access(ReadWrite)]
	config4: u8,
	#[access(NoAccess)]
	__reserved2: u8,
	/// Multiple interrupt select.
	#[access(ReadWrite)]
	mulint: le16,
	/// PCI revision ID.
	#[access(ReadOnly)]
	rerid: u8,
	#[access(NoAccess)]
	__reserved3: u8,
	/// Transmit status of all descriptors.
	#[access(ReadOnly)]
	tsad: le16,
	/// Basic mode control register.
	#[access(ReadWrite)]
	bmcr: le16,
	/// Basic mode status register.
	#[access(ReadOnly)]
	bmsr: le16,
	/// Auto-negotiation advertisement register.
	#[access(ReadWrite)]
	anar: le16,
	/// Auto-negotiation link partner register.
	#[access(ReadOnly)]
	anlpar: le16,
	/// Auto-negotiation expansion register.
	#[access(ReadOnly)]
	aner: le16,
	/// Disconnect counter.
	#[access(ReadOnly)]
	dis: le16,
	/// False carrier sense counter.
	#[access(ReadOnly)]
	fcsc: le16,
	/// N-way test register.
	#[access(ReadWrite)]
	nwaytr: le16,
	/// RX_ER counter.
	#[access(ReadOnly)]
	rec: le16,
	/// CS configuration register.
	#[access(ReadWrite)]
	cscr: le16,
	#[access(NoAccess)]
	__reserved4: u8,
	/// PHY parameter 1.
	#[access(ReadWrite)]
	phy1_parm: le32,
	/// Twister parameter.
	#[access(ReadWrite)]
	tw_parm: le32,
	/// PHY parameter 2.
	#[access(ReadWrite)]
	phy2_parm: u8,
	#[access(NoAccess)]
	__reserved5: [u8; 3],
	/// Power management CRC register0 for wakeup frame0.
	#[access(ReadWrite)]
	crc0: u8,
	/// Power management CRC register1 for wakeup frame1.
	#[access(ReadWrite)]
	crc1: u8,
	/// Power management CRC register2 for wakeup frame2.
	#[access(ReadWrite)]
	crc2: u8,
	/// Power management CRC register3 for wakeup frame3.
	#[access(ReadWrite)]
	crc3: u8,
	/// Power management CRC register4 for wakeup frame4.
	#[access(ReadWrite)]
	crc4: u8,
	/// Power management CRC register5 for wakeup frame5.
	#[access(ReadWrite)]
	crc5: u8,
	/// Power management CRC register6 for wakeup frame6.
	#[access(ReadWrite)]
	crc6: u8,
	/// Power management CRC register7 for wakeup frame7.
	#[access(ReadWrite)]
	crc7: u8,
	/// Power management wakeup frame0 (64-bit).
	#[access(ReadWrite)]
	wakeup0: le64,
	/// Power management wakeup frame1 (64-bit).
	#[access(ReadWrite)]
	wakeup1: le64,
	/// Power management wakeup frame2 (64-bit).
	#[access(ReadWrite)]
	wakeup2: le64,
	/// Power management wakeup frame3 (64-bit).
	#[access(ReadWrite)]
	wakeup3: le64,
	/// Power management wakeup frame4 (64-bit).
	#[access(ReadWrite)]
	wakeup4: le64,
	/// Power management wakeup frame5 (64-bit).
	#[access(ReadWrite)]
	wakeup5: le64,
	/// Power management wakeup frame6 (64-bit).
	#[access(ReadWrite)]
	wakeup6: le64,
	/// Power management wakeup frame7 (64-bit).
	#[access(ReadWrite)]
	wakeup7: le64,
	/// LSB of the mask byte of wakeup frame0 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc0: u8,
	/// LSB of the mask byte of wakeup frame1 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc1: u8,
	/// LSB of the mask byte of wakeup frame2 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc2: u8,
	/// LSB of the mask byte of wakeup frame3 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc3: u8,
	/// LSB of the mask byte of wakeup frame4 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc4: u8,
	/// LSB of the mask byte of wakeup frame5 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc5: u8,
	/// LSB of the mask byte of wakeup frame6 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc6: u8,
	/// LSB of the mask byte of wakeup frame7 within offset 12 to 75.
	#[access(ReadWrite)]
	lsbcrc7: u8,
	#[access(NoAccess)]
	__reserved6: [u8; 4],
	/// Configuration register 5.
	#[access(ReadWrite)]
	config5: u8,
	#[access(NoAccess)]
	__reserved7: [u8; 39],
}

#[derive(Error, Debug)]
pub enum RTL8139Error {
	#[error("initialization failed")]
	InitFailed,
	#[error("reset failed")]
	ResetFailed,
	#[error("unknown RTL8139 error")]
	Unknown,
}

struct RxFields {
	rxbuffer: Box<[u8], DeviceAlloc>,
	rxpos: usize,
	rx_in_use: bool,
}

impl RxFields {
	fn rx_peek_u16(&self) -> u16 {
		u16::from_le_bytes(
			self.rxbuffer[self.rxpos..][..mem::size_of::<u16>()]
				.try_into()
				.unwrap(),
		)
	}

	fn advance_rxpos(&mut self, count: usize) {
		self.rxpos += count;
		self.rxpos %= RX_BUF_LEN;
	}
}

impl RxToken<'_> {
	// Tells driver, that buffer is consumed and can be deallocated
	fn consume_current_buffer(&mut self) {
		let length = self.rx_fields.rx_peek_u16();
		self.rx_fields
			.advance_rxpos(usize::from(length) + mem::size_of::<u16>());

		// packets are dword aligned
		self.rx_fields.rxpos = ((self.rx_fields.rxpos + 3) & !0x3) % RX_BUF_LEN;

		let capr: u16 = if self.rx_fields.rxpos >= 0x10 {
			(self.rx_fields.rxpos - 0x10).try_into().unwrap()
		} else {
			(RX_BUF_LEN - (0x10 - self.rx_fields.rxpos))
				.try_into()
				.unwrap()
		};

		self.capr.write(le16::from(capr));
	}
}

impl Drop for RxToken<'_> {
	fn drop(&mut self) {
		self.rx_fields.rx_in_use = false;
	}
}

struct TxFields {
	tx_in_use: [bool; NO_TX_BUFFERS],
	tx_counter: usize,
	txbuffer: Box<[u8], DeviceAlloc>,
	remaining_bufs: usize,
}

/// RealTek RTL8139 network driver struct.
///
/// Struct allows to control device queues as also
/// the device itself.
pub(crate) struct RTL8139Driver {
	regs: VolatileRef<'static, Regs>,
	mtu: u16,
	irq: InterruptLine,
	mac: [u8; 6],
	rx_fields: RxFields,
	tx_fields: TxFields,
}

pub struct RxToken<'a> {
	capr: VolatilePtr<'a, le16>,
	rx_fields: &'a mut RxFields,
}

impl<'a> smoltcp::phy::RxToken for RxToken<'a> {
	fn consume<R, F>(mut self, f: F) -> R
	where
		F: FnOnce(&[u8]) -> R,
	{
		self.rx_fields.advance_rxpos(mem::size_of::<u16>());

		let length = self.rx_fields.rx_peek_u16() - 4; // copy packet (but not the CRC)
		let pos = (self.rx_fields.rxpos + mem::size_of::<u16>()) % RX_BUF_LEN;

		let mut vec_data = Vec::with_capacity(length as usize);

		// do we reach the end of the receive buffers?
		// in this case, we contact the two slices to one vec
		let frame = if pos + length as usize > RX_BUF_LEN {
			let first = &self.rx_fields.rxbuffer[pos..RX_BUF_LEN];
			let second = &self.rx_fields.rxbuffer[..length as usize - first.len()];

			vec_data.extend_from_slice(first);
			vec_data.extend_from_slice(second);
			vec_data.as_slice()
		} else {
			&self.rx_fields.rxbuffer[pos..][..length.into()]
		};

		let result = f(frame);

		self.consume_current_buffer();

		result
	}
}

pub struct TxToken<'a> {
	tsd0: VolatilePtr<'a, le32>,
	tsd1: VolatilePtr<'a, le32>,
	tsd2: VolatilePtr<'a, le32>,
	tsd3: VolatilePtr<'a, le32>,
	tx_fields: &'a mut TxFields,
}

impl Drop for TxToken<'_> {
	// For when the token is dropped without being used. When the token is consumed, the remaining buffer
	// count should only be increased after we receive confirmation of the actual transmission (i.e. TOK).
	fn drop(&mut self) {
		self.tx_fields.remaining_bufs += 1;
	}
}

impl<'a> smoltcp::phy::TxToken for TxToken<'a> {
	fn consume<R, F>(self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		let mut token = ManuallyDrop::new(self);
		let id = token.tx_fields.tx_counter % NO_TX_BUFFERS;

		assert!(
			!token.tx_fields.tx_in_use[id] && len <= TX_BUF_LEN,
			"Unable to get TX buffer"
		);

		token.tx_fields.tx_in_use[id] = true;
		token.tx_fields.tx_counter += 1;

		let buffer = &mut token.tx_fields.txbuffer[id * TX_BUF_LEN..][..len];
		let result = f(buffer);

		let len = le32::from(u32::try_from(len).unwrap());

		// send the packet
		match id {
			0 => token.tsd0.write(len),
			1 => token.tsd1.write(len),
			2 => token.tsd2.write(len),
			3 => token.tsd3.write(len),
			_ => unreachable!(),
		};

		result
	}
}

impl smoltcp::phy::Device for RTL8139Driver {
	type RxToken<'a> = RxToken<'a>;
	type TxToken<'a> = TxToken<'a>;

	fn receive(&mut self, _: smoltcp::time::Instant) -> Option<(RxToken<'_>, TxToken<'_>)> {
		if !self.rx_fields.rx_in_use && self.has_packet() {
			self.rx_fields.rx_in_use = true;
			let regs = self.regs.as_mut_ptr();

			Some((
				RxToken {
					capr: map_field!(regs.capr),
					rx_fields: &mut self.rx_fields,
				},
				TxToken {
					tsd0: map_field!(regs.tsd0),
					tsd1: map_field!(regs.tsd1),
					tsd2: map_field!(regs.tsd2),
					tsd3: map_field!(regs.tsd3),
					tx_fields: &mut self.tx_fields,
				},
			))
		} else {
			None
		}
	}

	fn transmit(&mut self, _: smoltcp::time::Instant) -> Option<TxToken<'_>> {
		if self.tx_fields.remaining_bufs > 0 {
			let regs = self.regs.as_mut_ptr();

			Some(TxToken {
				tsd0: map_field!(regs.tsd0),
				tsd1: map_field!(regs.tsd1),
				tsd2: map_field!(regs.tsd2),
				tsd3: map_field!(regs.tsd3),
				tx_fields: &mut self.tx_fields,
			})
		} else {
			None
		}
	}

	fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
		let mut device_capabilities = DeviceCapabilities::default();
		device_capabilities.medium = smoltcp::phy::Medium::Ethernet;
		device_capabilities.max_transmission_unit = usize::from(self.mtu);
		device_capabilities.max_burst_size = Some(usize::min(
			NO_TX_BUFFERS,
			RX_BUF_LEN / usize::from(self.mtu),
		));
		device_capabilities
	}
}

impl NetworkDriver for RTL8139Driver {
	/// Returns the MAC address of the network interface
	fn get_mac_address(&self) -> [u8; 6] {
		self.mac
	}

	fn has_packet(&self) -> bool {
		let cmd = self.regs.as_ptr().cr().read();

		if (cmd & CR_BUFE) != CR_BUFE {
			let header = self.rx_fields.rx_peek_u16();

			if header & ISR_ROK == ISR_ROK {
				return true;
			} else {
				warn!(
					"RTL8192: invalid header {:#x}, rx_pos {}\n",
					header, self.rx_fields.rxpos
				);
			}
		}

		false
	}

	fn set_polling_mode(&mut self, value: bool) {
		if value {
			self.regs
				.as_mut_ptr()
				.imr()
				.write(le16::from(INT_MASK_NO_ROK));
		} else {
			// Enable all known interrupts by setting the interrupt mask.
			self.regs.as_mut_ptr().imr().write(le16::from(INT_MASK));
		}
	}

	fn handle_interrupt(&mut self) {
		let isr_contents = self.regs.as_ptr().isr().read().to_ne();

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

		self.regs.as_mut_ptr().isr().write(le16::from(
			isr_contents & (ISR_RXOVW | ISR_TER | ISR_RER | ISR_TOK | ISR_ROK),
		));

		wake_network_waker();
	}
}

impl Driver for RTL8139Driver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"rtl8139"
	}
}

impl RTL8139Driver {
	fn tx_handler(&mut self) {
		for i in 0..self.tx_fields.tx_in_use.len() {
			if self.tx_fields.tx_in_use[i] {
				let txstatus = match i {
					0 => self.regs.as_ptr().tsd0().read().to_ne(),
					1 => self.regs.as_ptr().tsd1().read().to_ne(),
					2 => self.regs.as_ptr().tsd2().read().to_ne(),
					3 => self.regs.as_ptr().tsd3().read().to_ne(),
					_ => unreachable!(),
				};

				if (txstatus & (TSD_TABT | TSD_OWC)) > 0 {
					error!("RTL8139: major error");
					continue;
				}

				if (txstatus & TSD_TUN) == TSD_TUN {
					error!("RTL8139: transmit underrun");
				}

				if (txstatus & TSD_TOK) == TSD_TOK {
					self.tx_fields.tx_in_use[i] = false;
					self.tx_fields.remaining_bufs += 1;
				}
			}
		}
	}
}

impl Drop for RTL8139Driver {
	fn drop(&mut self) {
		debug!("Dropping RTL8129Driver!");

		// Software reset
		self.regs.as_mut_ptr().cr().write(CR_RST);
	}
}

pub(crate) fn init_device(
	device: &PciDevice<PciConfigRegion>,
) -> Result<RTL8139Driver, DriverError> {
	let irq = device.get_irq().unwrap();
	let mut regs = None;

	for i in 0..MAX_BARS {
		if let Some(Bar::Memory32 { .. }) = device.get_bar(i.try_into().unwrap()) {
			let (addr, _size) = device.memory_map_bar(i.try_into().unwrap(), true).unwrap();

			regs = Some(unsafe { VolatileRef::new(NonNull::new(addr.as_mut_ptr()).unwrap()) });
		}
	}

	let mut regs = regs.ok_or(DriverError::InitRTL8139DevFail(RTL8139Error::Unknown))?;

	debug!("Found RTL8139 at IO {regs:?} (irq {irq})");

	device.set_command(CommandRegister::BUS_MASTER_ENABLE);

	let mac = [
		regs.as_ptr().idr0().read(),
		regs.as_ptr().idr1().read(),
		regs.as_ptr().idr2().read(),
		regs.as_ptr().idr3().read(),
		regs.as_ptr().idr4().read(),
		regs.as_ptr().idr5().read(),
	];

	debug!(
		"MAC address {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
		mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
	);

	if regs.as_ptr().tcr().read().to_ne() == 0x00ff_ffffu32 {
		error!("Unable to initialize RTL8192");
		return Err(DriverError::InitRTL8139DevFail(RTL8139Error::InitFailed));
	}

	// Software reset
	regs.as_mut_ptr().cr().write(CR_RST);

	// The RST bit must be checked to make sure that the chip has finished the reset.
	// If the RST bit is high (1), then the reset is still in operation.
	let mut tmp: u16 = 10000;
	while (regs.as_ptr().cr().read() & CR_RST) == CR_RST && tmp > 0 {
		spin_loop();
		tmp -= 1;
	}

	if tmp == 0 {
		error!("RTL8139 reset failed");
		return Err(DriverError::InitRTL8139DevFail(RTL8139Error::ResetFailed));
	}

	// Enable Receive and Transmitter
	regs.as_mut_ptr().cr().write(CR_TE | CR_RE); // Sets the RE and TE bits high

	// lock config register
	regs.as_mut_ptr().cr_9346().write(CR9346_EEM1 | CR9346_EEM0);

	// clear all of CONFIG1
	regs.as_mut_ptr().config1().write(0);

	// disable driver loaded and lanwake bits, turn driver loaded bit back on
	regs.as_mut_ptr()
		.config1()
		.write(!(CONFIG1_DVRLOAD | CONFIG1_LWACT) | CONFIG1_DVRLOAD);

	// unlock config register
	regs.as_mut_ptr().cr_9346().write(0);

	// configure receive buffer
	// AB - Accept Broadcast: Accept broadcast packets sent to mac ff:ff:ff:ff:ff:ff
	// AM - Accept Multicast: Accept multicast packets.
	// APM - Accept Physical Match: Accept packets send to NIC's MAC address.
	// AAP - Accept All Packets. Accept all packets (run in promiscuous mode).
	regs.as_mut_ptr().rcr().write(le32::from(
		RCR_MXDMA2 | RCR_MXDMA1 | RCR_MXDMA0 | RCR_AB | RCR_AM | RCR_APM | RCR_AAP,
	)); // The WRAP bit isn't set!

	// set the transmit config register to
	// be the normal interframe gap time
	// set DMA max burst to 64bytes
	regs.as_mut_ptr()
		.tcr()
		.write(le32::from(TCR_IFG | TCR_MXDMA0 | TCR_MXDMA1 | TCR_MXDMA2));

	let rxbuffer = Box::new_zeroed_slice_in(RX_BUF_LEN, DeviceAlloc);
	let mut rxbuffer = unsafe { rxbuffer.assume_init() };
	let txbuffer = Box::new_zeroed_slice_in(NO_TX_BUFFERS * TX_BUF_LEN, DeviceAlloc);
	let mut txbuffer = unsafe { txbuffer.assume_init() };

	debug!("Allocate TxBuffer at {txbuffer:p} and RxBuffer at {rxbuffer:p}");

	let phys_addr = |p| le32::from(u32::try_from(DeviceAlloc.phys_addr_from(p).as_u64()).unwrap());

	// register the receive buffer
	regs.as_mut_ptr()
		.rbstart()
		.write(phys_addr(rxbuffer.as_mut_ptr()));

	// set each of the transmitter start address descriptors
	regs.as_mut_ptr()
		.tsad0()
		.write(phys_addr(txbuffer[..TX_BUF_LEN].as_mut_ptr()));
	regs.as_mut_ptr()
		.tsad1()
		.write(phys_addr(txbuffer[TX_BUF_LEN..][..TX_BUF_LEN].as_mut_ptr()));
	regs.as_mut_ptr().tsad2().write(phys_addr(
		txbuffer[2 * TX_BUF_LEN..][..TX_BUF_LEN].as_mut_ptr(),
	));
	regs.as_mut_ptr().tsad3().write(phys_addr(
		txbuffer[3 * TX_BUF_LEN..][..TX_BUF_LEN].as_mut_ptr(),
	));

	// Enable all known interrupts by setting the interrupt mask.
	regs.as_mut_ptr().imr().write(le16::from(INT_MASK));

	regs.as_mut_ptr().bmcr().write(le16::from(BMCR_ANE));
	let speed;
	let tmp = regs.as_ptr().bmcr().read().to_ne();
	if tmp & BMCR_SPD1000 == BMCR_SPD1000 {
		speed = 1000;
	} else if tmp & BMCR_SPD100 == BMCR_SPD100 {
		speed = 100;
	} else {
		speed = 10;
	}

	// Enable Receive and Transmitter
	regs.as_mut_ptr().cr().write(CR_TE | CR_RE); // Sets the RE and TE bits high

	info!(
		"RTL8139: CR = {:#x}, ISR = {:#x}, speed = {} mbps",
		regs.as_ptr().cr().read(),
		regs.as_ptr().isr().read(),
		speed
	);

	info!("RTL8139 use interrupt line {irq}");
	add_irq_name(irq, "rtl8139");

	Ok(RTL8139Driver {
		regs,
		mtu: mtu(),
		irq,
		mac,
		rx_fields: RxFields {
			rxbuffer,
			rxpos: 0,
			rx_in_use: false,
		},
		tx_fields: TxFields {
			tx_in_use: [false; NO_TX_BUFFERS],
			tx_counter: 0,
			txbuffer,
			remaining_bufs: NO_TX_BUFFERS,
		},
	})
}
