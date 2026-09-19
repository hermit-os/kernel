//! Driver for the Intel X550 10 GbE controller (ixgbe family).
//!
//! The initialization sequence and register usage follow the ixy.rs user space
//! driver (https://github.com/ixy-languages/ixy.rs) and the Intel X550
//! datasheet. In contrast to the 82599, the X550 performs link setup
//! (10GBASE-T auto-negotiation) in firmware, so no AUTOC configuration is
//! needed.

#![allow(dead_code)]

use alloc::boxed::Box;
use core::ptr::NonNull;
use core::sync::atomic::{Ordering, fence};

use memory_addresses::VirtAddr;
use pci_types::capability::PciCapability;
use pci_types::{CommandRegister, InterruptLine, MAX_BARS};
use smoltcp::phy::DeviceCapabilities;
use thiserror::Error;

use crate::arch::kernel::interrupts::*;
use crate::arch::kernel::pci::PciConfigRegion;
use crate::arch::kernel::processor::udelay;
use crate::drivers::error::DriverError;
use crate::drivers::net::{NetworkDriver, mtu};
use crate::drivers::pci::PciDevice;
use crate::drivers::pci::msix::{self, MsixTableVolatileElementAccess};
use crate::drivers::{Driver, InterruptHandlerMap};
use crate::executor::network::wake_network_waker;
use crate::mm::device_alloc::DeviceAlloc;

/// Number of descriptors per ring (must be a multiple of 8).
const RX_RING_SIZE: usize = 512;
const TX_RING_SIZE: usize = 512;
/// Size of one DMA packet buffer.
const BUF_SIZE: usize = 2048;

// General registers
const IXGBE_CTRL: u32 = 0x00000;
const IXGBE_CTRL_EXT: u32 = 0x00018;
const IXGBE_STATUS: u32 = 0x00008;
const IXGBE_EEC: u32 = 0x10010;

const IXGBE_CTRL_LNK_RST: u32 = 0x0000_0008;
const IXGBE_CTRL_RST: u32 = 0x0400_0000;
const IXGBE_CTRL_RST_MASK: u32 = IXGBE_CTRL_LNK_RST | IXGBE_CTRL_RST;
const IXGBE_CTRL_EXT_NS_DIS: u32 = 0x0001_0000;
const IXGBE_EEC_ARD: u32 = 0x0000_0200;

// Interrupt registers
const IXGBE_EICR: u32 = 0x00800;
const IXGBE_EIMS: u32 = 0x00880;
const IXGBE_EIMC: u32 = 0x00888;
const IXGBE_GPIE: u32 = 0x00898;
const IXGBE_GPIE_MSIX_MODE: u32 = 1 << 4;
const IXGBE_GPIE_PBA_SUPPORT: u32 = 1 << 31;
const fn ixgbe_ivar(i: u32) -> u32 {
	0x00900 + 4 * i
}

/// EICR bit of the interrupt vector queue 0 is mapped to.
const IXGBE_IRQ_MASK: u32 = 1 << 0;

// Receive registers
const IXGBE_RXCTRL: u32 = 0x03000;
const IXGBE_RDRXCTL: u32 = 0x02f00;
const IXGBE_FCTRL: u32 = 0x05080;
const IXGBE_HLREG0: u32 = 0x04240;

#[inline(always)]
const fn ixgbe_rxpbsize(i: u32) -> u32 {
	0x03c00 + 4 * i
}

#[inline(always)]
const fn ixgbe_srrctl(i: u32) -> u32 {
	0x01014 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_rdbal(i: u32) -> u32 {
	0x01000 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_rdbah(i: u32) -> u32 {
	0x01004 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_rdlen(i: u32) -> u32 {
	0x01008 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_rdh(i: u32) -> u32 {
	0x01010 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_rdt(i: u32) -> u32 {
	0x01018 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_rxdctl(i: u32) -> u32 {
	0x01028 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_dca_rxctrl(i: u32) -> u32 {
	0x0100c + 0x40 * i
}

const IXGBE_RXCTRL_RXEN: u32 = 0x0000_0001;
const IXGBE_RDRXCTL_DMAIDONE: u32 = 0x0000_0008;
const IXGBE_RDRXCTL_CRCSTRIP: u32 = 0x0000_0002;
const IXGBE_FCTRL_BAM: u32 = 0x0000_0400;
const IXGBE_FCTRL_UPE: u32 = 0x0000_0200;
const IXGBE_FCTRL_MPE: u32 = 0x0000_0100;
const IXGBE_HLREG0_RXCRCSTRP: u32 = 0x0000_0002;
const IXGBE_HLREG0_TXCRCEN: u32 = 0x0000_0001;
const IXGBE_HLREG0_TXPADEN: u32 = 0x0000_0400;
const IXGBE_RXPBSIZE_128KB: u32 = 0x0002_0000;
const IXGBE_SRRCTL_DESCTYPE_MASK: u32 = 0x0e00_0000;
const IXGBE_SRRCTL_DESCTYPE_ADV_ONEBUF: u32 = 0x0200_0000;
const IXGBE_SRRCTL_DROP_EN: u32 = 0x1000_0000;
const IXGBE_SRRCTL_BSIZEPKT_MASK: u32 = 0x0000_001f;
const IXGBE_RXDCTL_ENABLE: u32 = 0x0200_0000;
const IXGBE_DCA_RXCTRL_DESC_RRO_EN: u32 = 1 << 12;

// Transmit registers
const IXGBE_DMATXCTL: u32 = 0x04a80;
const IXGBE_DTXMXSZRQ: u32 = 0x08100;
const IXGBE_RTTDCS: u32 = 0x04900;

#[inline(always)]
const fn ixgbe_txpbsize(i: u32) -> u32 {
	0x0cc00 + 4 * i
}

#[inline(always)]
const fn ixgbe_tdbal(i: u32) -> u32 {
	0x06000 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_tdbah(i: u32) -> u32 {
	0x06004 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_tdlen(i: u32) -> u32 {
	0x06008 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_tdh(i: u32) -> u32 {
	0x06010 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_tdt(i: u32) -> u32 {
	0x06018 + 0x40 * i
}

#[inline(always)]
const fn ixgbe_txdctl(i: u32) -> u32 {
	0x06028 + 0x40 * i
}

const IXGBE_DMATXCTL_TE: u32 = 0x0000_0001;
const IXGBE_RTTDCS_ARBDIS: u32 = 0x0000_0040;
const IXGBE_TXPBSIZE_40KB: u32 = 0x0000_a000;
const IXGBE_TXDCTL_ENABLE: u32 = 0x0200_0000;

// MAC address and link
#[inline(always)]
const fn ixgbe_ral(i: u32) -> u32 {
	0x0a200 + 8 * i
}

#[inline(always)]
const fn ixgbe_rah(i: u32) -> u32 {
	0x0a204 + 8 * i
}

const IXGBE_LINKS: u32 = 0x042a4;
const IXGBE_LINKS_UP: u32 = 0x4000_0000;
const IXGBE_LINKS_SPEED_MASK: u32 = 0x3000_0000;
const IXGBE_LINKS_SPEED_10G: u32 = 0x3000_0000;
const IXGBE_LINKS_SPEED_1G: u32 = 0x2000_0000;
const IXGBE_LINKS_SPEED_100M: u32 = 0x1000_0000;

// Statistic registers (read to clear)
const IXGBE_GPRC: u32 = 0x04074;
const IXGBE_GPTC: u32 = 0x04080;
const IXGBE_GORCL: u32 = 0x04088;
const IXGBE_GORCH: u32 = 0x0408c;
const IXGBE_GOTCL: u32 = 0x04090;
const IXGBE_GOTCH: u32 = 0x04094;

// Advanced receive descriptor (write-back layout)
const IXGBE_RXDADV_STAT_DD: u32 = 0x0000_0001;
const IXGBE_RXDADV_STAT_EOP: u32 = 0x0000_0002;

// Advanced transmit descriptor
const IXGBE_ADVTXD_DTYP_DATA: u32 = 0x0030_0000;
const IXGBE_ADVTXD_DCMD_DEXT: u32 = 0x2000_0000;
const IXGBE_ADVTXD_DCMD_EOP: u32 = 0x0100_0000;
const IXGBE_ADVTXD_DCMD_RS: u32 = 0x0800_0000;
const IXGBE_ADVTXD_DCMD_IFCS: u32 = 0x0200_0000;
const IXGBE_ADVTXD_STAT_DD: u32 = 0x0000_0001;
const IXGBE_ADVTXD_PAYLEN_SHIFT: u32 = 14;

/// Advanced (16-byte) descriptor, used for both rx and tx.
///
/// Read format of the rx descriptor: `lo` = packet buffer address, `hi` =
/// header buffer address. Write-back format: bits 0..32 of `hi` contain the
/// extended status (DD/EOP), bits 32..48 the packet length.
///
/// Tx read format: `lo` = buffer address, `hi` = `cmd_type_len` (bits 0..32)
/// and `olinfo_status` (bits 32..64). On write-back the DD bit is set in bit
/// 32 of `hi`.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct AdvDesc {
	lo: u64,
	hi: u64,
}

#[derive(Error, Debug)]
pub enum IxgbeError {
	#[error("no usable memory BAR found")]
	NoBar,
	#[error("device reset failed")]
	ResetFailed,
	#[error("initialization failed")]
	InitFailed,
	#[error("unknown ixgbe error")]
	Unknown,
}

/// Thin wrapper for volatile access to the memory-mapped BAR0 registers.
#[derive(Clone, Copy)]
struct Regs {
	base: NonNull<u8>,
}

// The register window is exclusively owned by the driver.
unsafe impl Send for Regs {}

impl Regs {
	fn read(&self, reg: u32) -> u32 {
		unsafe {
			self.base
				.as_ptr()
				.add(reg as usize)
				.cast::<u32>()
				.read_volatile()
		}
	}

	fn write(&self, reg: u32, value: u32) {
		unsafe {
			self.base
				.as_ptr()
				.add(reg as usize)
				.cast::<u32>()
				.write_volatile(value);
		}
	}

	fn set_flags(&self, reg: u32, flags: u32) {
		self.write(reg, self.read(reg) | flags);
	}

	fn clear_flags(&self, reg: u32, flags: u32) {
		self.write(reg, self.read(reg) & !flags);
	}

	/// Busy-waits until all bits in `mask` are set, up to ~1 s.
	fn wait_set(&self, reg: u32, mask: u32) -> Result<(), IxgbeError> {
		for _ in 0..10_000 {
			if self.read(reg) & mask == mask {
				return Ok(());
			}
			udelay(100);
		}
		Err(IxgbeError::InitFailed)
	}
}

struct RxRing {
	descs: Box<[AdvDesc], DeviceAlloc>,
	bufs: Box<[u8], DeviceAlloc>,
	/// Next descriptor to check for a received packet.
	index: usize,
}

impl RxRing {
	fn desc_ptr(&mut self, i: usize) -> *mut AdvDesc {
		&raw mut self.descs[i]
	}

	fn buf_phys(&mut self, i: usize) -> u64 {
		DeviceAlloc
			.phys_addr_from(self.bufs[i * BUF_SIZE..].as_mut_ptr())
			.as_u64()
	}

	/// Returns the write-back status of descriptor `i`.
	fn status(&self, i: usize) -> u32 {
		let hi = unsafe { (&raw const self.descs[i].hi).read_volatile() };
		hi as u32
	}

	fn has_packet(&self) -> bool {
		self.status(self.index) & IXGBE_RXDADV_STAT_DD != 0
	}

	/// Hands descriptor `i` back to the hardware.
	fn rearm(&mut self, i: usize) {
		let phys = self.buf_phys(i);
		let desc = self.desc_ptr(i);
		unsafe {
			(&raw mut (*desc).lo).write_volatile(phys);
			(&raw mut (*desc).hi).write_volatile(0);
		}
	}
}

struct TxRing {
	descs: Box<[AdvDesc], DeviceAlloc>,
	bufs: Box<[u8], DeviceAlloc>,
	/// Next descriptor to use for sending.
	index: usize,
	/// Next descriptor to check for completed transmission.
	clean_index: usize,
}

impl TxRing {
	fn desc_ptr(&mut self, i: usize) -> *mut AdvDesc {
		&raw mut self.descs[i]
	}

	fn buf_phys(&mut self, i: usize) -> u64 {
		DeviceAlloc
			.phys_addr_from(self.bufs[i * BUF_SIZE..].as_mut_ptr())
			.as_u64()
	}

	/// Reclaims descriptors whose transmission has completed.
	fn clean(&mut self) {
		while self.clean_index != self.index {
			let clean_index = self.clean_index;
			let status = unsafe { (&raw mut (*self.desc_ptr(clean_index)).hi).read_volatile() };
			if (status >> 32) as u32 & IXGBE_ADVTXD_STAT_DD == 0 {
				break;
			}
			self.clean_index = (clean_index + 1) % TX_RING_SIZE;
		}
	}

	fn is_full(&self) -> bool {
		(self.index + 1) % TX_RING_SIZE == self.clean_index
	}
}

/// Driver for the Intel X550 network controller.
pub(crate) struct IxgbeDriver {
	regs: Regs,
	mac: [u8; 6],
	mtu: u16,
	rx: RxRing,
	tx: TxRing,
}

pub struct RxToken<'a> {
	rx: &'a mut RxRing,
	regs: Regs,
}

impl smoltcp::phy::RxToken for RxToken<'_> {
	fn consume<R, F>(self, f: F) -> R
	where
		F: FnOnce(&[u8]) -> R,
	{
		let index = self.rx.index;
		let status = self.rx.status(index);
		// The write-back layout stores the packet length in bits 32..48 of `hi`.
		let hi = unsafe { (&raw mut (*self.rx.desc_ptr(index)).hi).read_volatile() };
		let len = ((hi >> 32) & 0xffff) as usize;

		if status & IXGBE_RXDADV_STAT_EOP == 0 {
			warn!("ixgbe: multi-descriptor packet received, truncating");
		}

		fence(Ordering::Acquire);
		let frame = &self.rx.bufs[index * BUF_SIZE..][..len.min(BUF_SIZE)];
		let result = f(frame);

		// Hand the descriptor back to the hardware and advance the tail.
		self.rx.rearm(index);
		self.rx.index = (index + 1) % RX_RING_SIZE;
		fence(Ordering::Release);
		self.regs.write(ixgbe_rdt(0), index as u32);

		result
	}
}

pub struct TxToken<'a> {
	tx: &'a mut TxRing,
	regs: Regs,
}

impl smoltcp::phy::TxToken for TxToken<'_> {
	fn consume<R, F>(self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		assert!(len <= BUF_SIZE, "ixgbe: packet too large for tx buffer");

		let index = self.tx.index;
		let result = f(&mut self.tx.bufs[index * BUF_SIZE..][..len]);

		let phys = self.tx.buf_phys(index);
		let cmd_type_len = IXGBE_ADVTXD_DCMD_EOP
			| IXGBE_ADVTXD_DCMD_RS
			| IXGBE_ADVTXD_DCMD_IFCS
			| IXGBE_ADVTXD_DCMD_DEXT
			| IXGBE_ADVTXD_DTYP_DATA
			| len as u32;
		let olinfo_status = (len as u32) << IXGBE_ADVTXD_PAYLEN_SHIFT;

		let desc = self.tx.desc_ptr(index);
		fence(Ordering::Release);
		unsafe {
			(&raw mut (*desc).lo).write_volatile(phys);
			(&raw mut (*desc).hi)
				.write_volatile(u64::from(cmd_type_len) | (u64::from(olinfo_status) << 32));
		}

		self.tx.index = (index + 1) % TX_RING_SIZE;
		self.regs.write(ixgbe_tdt(0), self.tx.index as u32);

		result
	}
}

impl smoltcp::phy::Device for IxgbeDriver {
	type RxToken<'a> = RxToken<'a>;
	type TxToken<'a> = TxToken<'a>;

	fn receive(&mut self, _: smoltcp::time::Instant) -> Option<(RxToken<'_>, TxToken<'_>)> {
		if !self.rx.has_packet() {
			return None;
		}

		self.tx.clean();
		if self.tx.is_full() {
			return None;
		}

		Some((
			RxToken {
				rx: &mut self.rx,
				regs: self.regs,
			},
			TxToken {
				tx: &mut self.tx,
				regs: self.regs,
			},
		))
	}

	fn transmit(&mut self, _: smoltcp::time::Instant) -> Option<TxToken<'_>> {
		self.tx.clean();
		if self.tx.is_full() {
			return None;
		}

		Some(TxToken {
			tx: &mut self.tx,
			regs: self.regs,
		})
	}

	fn capabilities(&self) -> DeviceCapabilities {
		let mut device_capabilities = DeviceCapabilities::default();
		device_capabilities.medium = smoltcp::phy::Medium::Ethernet;
		device_capabilities.max_transmission_unit = usize::from(self.mtu);
		device_capabilities.max_burst_size = Some(TX_RING_SIZE / 2);
		device_capabilities
	}
}

impl NetworkDriver for IxgbeDriver {
	/// Returns the MAC address of the network interface
	fn get_mac_address(&self) -> [u8; 6] {
		self.mac
	}

	fn has_packet(&self) -> bool {
		self.rx.has_packet()
	}

	fn set_polling_mode(&mut self, value: bool) {
		if value {
			self.regs.write(IXGBE_EIMC, IXGBE_IRQ_MASK);
		} else {
			// Clear stale causes before re-enabling the interrupt.
			self.regs.write(IXGBE_EICR, IXGBE_IRQ_MASK);
			self.regs.write(IXGBE_EIMS, IXGBE_IRQ_MASK);
		}
	}

	fn handle_interrupt(&mut self) {
		// Reading EICR clears the interrupt causes in legacy/MSI mode. Under
		// MSI-X they have to be written back instead, which does no harm in the
		// other modes because the register clears on write in all of them.
		let eicr = self.regs.read(IXGBE_EICR);
		self.regs.write(IXGBE_EICR, eicr);

		self.tx.clean();
		wake_network_waker();
	}
}

impl Driver for IxgbeDriver {
	fn get_name() -> &'static str {
		"ixgbe"
	}
}

impl Drop for IxgbeDriver {
	fn drop(&mut self) {
		debug!("Dropping IxgbeDriver!");

		// Disable interrupts and both DMA engines.
		self.regs.write(IXGBE_EIMC, 0x7fff_ffff);
		self.regs.clear_flags(IXGBE_RXCTRL, IXGBE_RXCTRL_RXEN);
		self.regs.clear_flags(ixgbe_txdctl(0), IXGBE_TXDCTL_ENABLE);
		self.regs.clear_flags(ixgbe_rxdctl(0), IXGBE_RXDCTL_ENABLE);
	}
}

/// Allocates a descriptor ring and its packet buffers.
fn alloc_ring(entries: usize) -> (Box<[AdvDesc], DeviceAlloc>, Box<[u8], DeviceAlloc>) {
	let descs = unsafe { Box::new_zeroed_slice_in(entries, DeviceAlloc).assume_init() };
	let bufs = unsafe { Box::new_zeroed_slice_in(entries * BUF_SIZE, DeviceAlloc).assume_init() };
	(descs, bufs)
}

/// Switches the device over to message signaled interrupts and returns the
/// interrupt line its single vector is mapped to.
fn setup_msix(
	device: &PciDevice<PciConfigRegion>,
	bars: &[Option<(VirtAddr, usize)>; MAX_BARS],
	regs: &Regs,
	handlers: &InterruptHandlerMap,
) -> Option<InterruptLine> {
	let mut capability = device
		.capabilities()?
		.find_map(|capability| match capability {
			PciCapability::MsiX(capability) => Some(capability),
			_ => None,
		})?;

	capability.set_enabled(true, device.access());

	// The capability names the bar the table lives in, which is not necessarily
	// the one holding the registers.
	let (base_address, _) = bars[usize::from(capability.table_bar())]?;
	let table = NonNull::slice_from_raw_parts(
		NonNull::with_exposed_provenance(core::num::NonZero::new(
			base_address.as_usize() + usize::try_from(capability.table_offset()).unwrap(),
		)?),
		capability.table_size().into(),
	);
	let mut table = unsafe { volatile::VolatileRef::<'_, [msix::TableEntry]>::new(table) };

	// The driver operates a single receive and transmit queue, so one vector
	// suffices. Interrupt lines are mapped to vectors by adding 32.
	let irq = (0..=(msix::VECTOR_MAX - 32)).find(|line| !handlers.contains_key(line))?;
	table.configure(0, irq + 32);

	// Switch the interrupt logic from legacy/MSI over to MSI-X. The IVAR entries
	// keep their meaning: their allocation field selects the MSI-X vector now
	// instead of an EICR bit, and queue 0 uses index 0 either way.
	regs.set_flags(IXGBE_GPIE, IXGBE_GPIE_MSIX_MODE | IXGBE_GPIE_PBA_SUPPORT);

	info!("ixgbe uses MSI-X vector 0 of {}", capability.table_size());

	Some(irq)
}

pub(crate) fn init_device(
	device: &PciDevice<PciConfigRegion>,
	handlers: &mut InterruptHandlerMap,
) -> Result<IxgbeDriver, DriverError> {
	let irq = device.get_irq();

	// Map every bar in one go, so that the msi-x table stays reachable no matter
	// which bar it lives in. I/O bars are left out by the mapping, which makes
	// the first entry the first memory bar, holding the registers.
	let bars = device.memory_map_bars(true);
	let (addr, size) = bars
		.iter()
		.flatten()
		.copied()
		.next()
		.ok_or(DriverError::InitIxgbeDevFail(IxgbeError::NoBar))?;
	let regs = Regs {
		base: NonNull::new(addr.as_mut_ptr()).unwrap(),
	};

	info!("Found ixgbe device with BAR0 at {addr:p} (size {size:#x}, irq {irq:?})");

	device.set_command(CommandRegister::BUS_MASTER_ENABLE | CommandRegister::MEMORY_ENABLE);

	// disable all interrupts during initialization
	regs.write(IXGBE_EIMC, 0x7fff_ffff);

	// Global reset (link reset + software reset)
	regs.write(IXGBE_CTRL, IXGBE_CTRL_RST_MASK);
	for _ in 0..10_000 {
		if regs.read(IXGBE_CTRL) & IXGBE_CTRL_RST_MASK == 0 {
			break;
		}
		udelay(100);
	}
	if regs.read(IXGBE_CTRL) & IXGBE_CTRL_RST_MASK != 0 {
		error!("ixgbe reset failed");
		return Err(DriverError::InitIxgbeDevFail(IxgbeError::ResetFailed));
	}
	// 10 ms delay after the reset.
	udelay(10_000);

	// Interrupts are re-enabled by the reset - disable them again.
	regs.write(IXGBE_EIMC, 0x7fff_ffff);

	// Wait for EEPROM auto read completion
	regs.wait_set(IXGBE_EEC, IXGBE_EEC_ARD)
		.map_err(DriverError::InitIxgbeDevFail)?;

	// Wait for DMA initialization to complete
	regs.wait_set(IXGBE_RDRXCTL, IXGBE_RDRXCTL_DMAIDONE)
		.map_err(DriverError::InitIxgbeDevFail)?;

	// Read the MAC address that firmware loaded from the EEPROM.
	let ral = regs.read(ixgbe_ral(0));
	let rah = regs.read(ixgbe_rah(0));
	let mac = [
		ral as u8,
		(ral >> 8) as u8,
		(ral >> 16) as u8,
		(ral >> 24) as u8,
		rah as u8,
		(rah >> 8) as u8,
	];
	debug!(
		"MAC address {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
		mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
	);

	// Reset the (read-to-clear) statistic counters.
	regs.read(IXGBE_GPRC);
	regs.read(IXGBE_GPTC);
	regs.read(IXGBE_GORCL);
	regs.read(IXGBE_GORCH);
	regs.read(IXGBE_GOTCL);
	regs.read(IXGBE_GOTCH);

	// Section 4.6.7 - receive initialization
	regs.clear_flags(IXGBE_RXCTRL, IXGBE_RXCTRL_RXEN);

	// 128 KiB packet buffer for queue 0, no DCB/VT
	regs.write(ixgbe_rxpbsize(0), IXGBE_RXPBSIZE_128KB);
	for i in 1..8 {
		regs.write(ixgbe_rxpbsize(i), 0);
	}

	// Strip the ethernet CRC in hardware.
	regs.set_flags(IXGBE_HLREG0, IXGBE_HLREG0_RXCRCSTRP);
	regs.set_flags(IXGBE_RDRXCTL, IXGBE_RDRXCTL_CRCSTRIP);

	// Accept broadcasts and run in promiscuous mode; smoltcp filters by MAC.
	regs.set_flags(
		IXGBE_FCTRL,
		IXGBE_FCTRL_BAM | IXGBE_FCTRL_UPE | IXGBE_FCTRL_MPE,
	);

	let (mut rx_descs, rx_bufs) = alloc_ring(RX_RING_SIZE);
	let (tx_descs, tx_bufs) = alloc_ring(TX_RING_SIZE);

	// Configure rx queue 0: advanced one-buffer descriptors, 2 KiB buffers,
	// drop packets if no descriptors are available.
	let mut srrctl = regs.read(ixgbe_srrctl(0));
	srrctl &= !(IXGBE_SRRCTL_DESCTYPE_MASK | IXGBE_SRRCTL_BSIZEPKT_MASK);
	srrctl |= IXGBE_SRRCTL_DESCTYPE_ADV_ONEBUF | IXGBE_SRRCTL_DROP_EN | (BUF_SIZE / 1024) as u32;
	regs.write(ixgbe_srrctl(0), srrctl);

	let rx_ring_phys = DeviceAlloc.phys_addr_from(rx_descs.as_mut_ptr()).as_u64();
	regs.write(ixgbe_rdbal(0), rx_ring_phys as u32);
	regs.write(ixgbe_rdbah(0), (rx_ring_phys >> 32) as u32);
	regs.write(ixgbe_rdlen(0), (RX_RING_SIZE * size_of::<AdvDesc>()) as u32);
	regs.write(ixgbe_rdh(0), 0);
	regs.write(ixgbe_rdt(0), 0);

	// Section 4.6.7 - CRC offload and no-snoop need to be set
	regs.set_flags(IXGBE_CTRL_EXT, IXGBE_CTRL_EXT_NS_DIS);
	// This flag probably refers to a broken feature: it is reserved on newer
	// datasheet revisions but ixy and the Linux driver clear it anyway.
	regs.clear_flags(ixgbe_dca_rxctrl(0), IXGBE_DCA_RXCTRL_DESC_RRO_EN);

	regs.set_flags(IXGBE_RXCTRL, IXGBE_RXCTRL_RXEN);

	let mut rx = RxRing {
		descs: rx_descs,
		bufs: rx_bufs,
		index: 0,
	};

	// Fill the ring with buffer addresses and hand it to the hardware.
	for i in 0..RX_RING_SIZE {
		rx.rearm(i);
	}
	regs.set_flags(ixgbe_rxdctl(0), IXGBE_RXDCTL_ENABLE);
	regs.wait_set(ixgbe_rxdctl(0), IXGBE_RXDCTL_ENABLE)
		.map_err(DriverError::InitIxgbeDevFail)?;
	regs.write(ixgbe_rdt(0), (RX_RING_SIZE - 1) as u32);

	// Section 4.6.8 - transmit initialization
	// CRC insertion and padding of short frames
	regs.set_flags(IXGBE_HLREG0, IXGBE_HLREG0_TXCRCEN | IXGBE_HLREG0_TXPADEN);

	// 40 KiB packet buffer for queue 0
	regs.write(ixgbe_txpbsize(0), IXGBE_TXPBSIZE_40KB);
	for i in 1..8 {
		regs.write(ixgbe_txpbsize(i), 0);
	}

	// Required when not using DCB/VTd
	regs.write(IXGBE_DTXMXSZRQ, 0xfff);
	regs.clear_flags(IXGBE_RTTDCS, IXGBE_RTTDCS_ARBDIS);

	let mut tx = TxRing {
		descs: tx_descs,
		bufs: tx_bufs,
		index: 0,
		clean_index: 0,
	};

	let tx_ring_phys = DeviceAlloc.phys_addr_from(tx.descs.as_mut_ptr()).as_u64();
	regs.write(ixgbe_tdbal(0), tx_ring_phys as u32);
	regs.write(ixgbe_tdbah(0), (tx_ring_phys >> 32) as u32);
	regs.write(ixgbe_tdlen(0), (TX_RING_SIZE * size_of::<AdvDesc>()) as u32);
	regs.write(ixgbe_tdh(0), 0);
	regs.write(ixgbe_tdt(0), 0);

	// Descriptor write-back thresholds; see ixy and section 8.2.3.9.10.
	let mut txdctl = regs.read(ixgbe_txdctl(0));
	txdctl &= !(0x7f | (0x7f << 8) | (0x7f << 16));
	txdctl |= 36 | (8 << 8) | (4 << 16);
	regs.write(ixgbe_txdctl(0), txdctl);

	regs.set_flags(IXGBE_DMATXCTL, IXGBE_DMATXCTL_TE);
	regs.set_flags(ixgbe_txdctl(0), IXGBE_TXDCTL_ENABLE);
	regs.wait_set(ixgbe_txdctl(0), IXGBE_TXDCTL_ENABLE)
		.map_err(DriverError::InitIxgbeDevFail)?;

	// Map rx queue 0 and tx queue 0 to interrupt vector 0 (EICR bit 0) for
	// legacy/MSI interrupts. Bit 7 of each field marks the entry as valid.
	regs.write(ixgbe_ivar(0), 0x0000_8080);

	// Prefer MSI-X over the legacy interrupt whenever the device offers it.
	let irq = setup_msix(device, &bars, &regs, handlers).or(irq);

	regs.write(IXGBE_EICR, IXGBE_IRQ_MASK);
	regs.write(IXGBE_EIMS, IXGBE_IRQ_MASK);

	// Wait a bit for the link. 10GBASE-T auto-negotiation can take multiple
	// seconds; the driver works fine if the link comes up later.
	let mut link = 0;
	for _ in 0..300 {
		link = regs.read(IXGBE_LINKS);
		if link & IXGBE_LINKS_UP != 0 {
			break;
		}
		udelay(10_000);
	}
	if link & IXGBE_LINKS_UP != 0 {
		let speed = match link & IXGBE_LINKS_SPEED_MASK {
			IXGBE_LINKS_SPEED_10G => "10 Gbit/s",
			IXGBE_LINKS_SPEED_1G => "1 Gbit/s",
			IXGBE_LINKS_SPEED_100M => "100 Mbit/s",
			_ => "unknown speed",
		};
		info!("ixgbe: link is up at {speed}");
	} else {
		warn!("ixgbe: no link established (yet)");
	}

	if let Some(irq) = irq {
		info!("ixgbe uses interrupt line {irq}");
		handlers
			.entry(irq)
			.or_default()
			.push_back(crate::executor::network::network_handler);
		add_irq_name(irq, "ixgbe");
	} else {
		// A passed-through device is not guaranteed to offer a legacy interrupt.
		// It only makes progress if it is polled, which the `idle-poll` feature
		// takes care of.
		warn!("ixgbe has no legacy interrupt, the device needs to be polled");
	}

	Ok(IxgbeDriver {
		regs,
		mac,
		mtu: mtu(),
		rx,
		tx,
	})
}
