//! This module contains the implementation of the Bochs Graphics Adapter (BGA) driver.
//!
//! The driver uses the Bochs VBE Extensions, which use two I/O ports to communicate with the
//! emulated VGA card instead of relying on a 16-bit VBE BIOS. The driver initializes the BGA
//! device, sets the desired resolution and bits per pixel (BPP), and maps the framebuffer
//! into the virtual address space. It also provides a function to retrieve the physical
//! address of the framebuffer.

use hermit_sync::OnceCell;
use memory_addresses::{PhysAddr, VirtAddr};
use pci_types::{Bar, CommandRegister};
use x86_64::instructions::port::{Port, PortWriteOnly};

use crate::arch::x86_64::mm::paging::{
	self, BasePageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::drivers::pci::PciDevice;
use crate::kernel::pci::PciConfigRegion;

pub struct BgaInfo {
	pub framebuffer: *mut u8,
	pub width: u16,
	pub height: u16,
	pub bpp: u16,
}

static BGA_INFO: OnceCell<BgaInfo> = OnceCell::new();

unsafe impl Send for BgaInfo {}
unsafe impl Sync for BgaInfo {}

const VBE_DISPI_IOPORT_INDEX: u16 = 0x01ce;
const VBE_DISPI_IOPORT_DATA: u16 = 0x01cf;

pub struct VbeDispiIndex;

#[allow(dead_code)]
impl VbeDispiIndex {
	#[doc(alias = "VBE_DISPI_INDEX_ID")]
	pub const ID: u16 = 0;
	#[doc(alias = "VBE_DISPI_INDEX_XRES")]
	pub const XRES: u16 = 1;
	#[doc(alias = "VBE_DISPI_INDEX_YRES")]
	pub const YRES: u16 = 2;
	#[doc(alias = "VBE_DISPI_INDEX_BPP")]
	pub const BPP: u16 = 3;
	#[doc(alias = "VBE_DISPI_INDEX_ENABLE")]
	pub const ENABLE: u16 = 4;
	#[doc(alias = "VBE_DISPI_INDEX_BANK")]
	pub const BANK: u16 = 5;
	#[doc(alias = "VBE_DISPI_INDEX_VIRT_WIDTH")]
	pub const VIRT_WIDTH: u16 = 6;
	#[doc(alias = "VBE_DISPI_INDEX_VIRT_HEIGHT")]
	pub const VIRT_HEIGHT: u16 = 7;
	#[doc(alias = "VBE_DISPI_INDEX_X_OFFSET")]
	pub const X_OFFSET: u16 = 8;
	#[doc(alias = "VBE_DISPI_INDEX_Y_OFFSET")]
	pub const Y_OFFSET: u16 = 9;
}

const VBE_DISPI_DISABLED: u16 = 0x00;
const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;

#[allow(dead_code)]
const VBE_DISPI_NOCLEARMEM: u16 = 0x80;

pub struct VbeDispiId;

#[allow(dead_code)]
impl VbeDispiId {
	#[doc(alias = "VBE_DISPI_ID0")]
	pub const ID0: u16 = 0xb0c0;
	#[doc(alias = "VBE_DISPI_ID1")]
	pub const ID1: u16 = 0xb0c1;
	#[doc(alias = "VBE_DISPI_ID2")]
	pub const ID2: u16 = 0xb0c2;
	#[doc(alias = "VBE_DISPI_ID3")]
	pub const ID3: u16 = 0xb0c3;
	#[doc(alias = "VBE_DISPI_ID4")]
	pub const ID4: u16 = 0xb0c4;
	#[doc(alias = "VBE_DISPI_ID5")]
	pub const ID5: u16 = 0xb0c5;
}

struct BgaRegisters;

impl BgaRegisters {
	pub fn read(index: u16) -> u16 {
		let mut index_port: PortWriteOnly<u16> = PortWriteOnly::new(VBE_DISPI_IOPORT_INDEX);
		let mut data_port: Port<u16> = Port::new(VBE_DISPI_IOPORT_DATA);
		unsafe {
			index_port.write(index);
			data_port.read()
		}
	}

	pub fn write(index: u16, value: u16) {
		let mut index_port: PortWriteOnly<u16> = PortWriteOnly::new(VBE_DISPI_IOPORT_INDEX);
		let mut data_port: Port<u16> = Port::new(VBE_DISPI_IOPORT_DATA);
		unsafe {
			index_port.write(index);
			data_port.write(value);
		}
	}
}

pub fn init_device(adapter: &PciDevice<PciConfigRegion>) {
	//To Do: Add support for different resolutions and BPP values
	let width: u16 = 640;
	let height: u16 = 400;
	let bpp: u16 = 32;

	let bga_version = BgaRegisters::read(VbeDispiIndex::ID);

	if bga_version != VbeDispiId::ID5 {
		error!("Unsupported BGA version: {bga_version:#06x}");
		return;
	}

	BgaRegisters::write(VbeDispiIndex::ENABLE, VBE_DISPI_DISABLED);
	BgaRegisters::write(VbeDispiIndex::XRES, width);
	BgaRegisters::write(VbeDispiIndex::YRES, height);
	BgaRegisters::write(VbeDispiIndex::BPP, bpp);
	BgaRegisters::write(
		VbeDispiIndex::ENABLE,
		VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
	);

	adapter.set_command(CommandRegister::MEMORY_ENABLE);

	let (phys_addr, size) = match adapter.get_bar(0) {
		Some(Bar::Memory32 { address, size, .. }) => (u64::from(address), size as usize),
		Some(Bar::Memory64 { address, size, .. }) => (address, size as usize),
		_ => return,
	};

	BGA_INFO
		.set(BgaInfo {
			framebuffer: core::ptr::with_exposed_provenance_mut(phys_addr as usize),
			width,
			height,
			bpp,
		})
		.ok();

	assert!(
		size.is_multiple_of(4096),
		"Framebuffer size must be a multiple of 4096 bytes"
	);
	let page_count = size / 4096;

	let mut flags = PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	paging::map::<BasePageSize>(
		VirtAddr::new(phys_addr),
		PhysAddr::new(phys_addr),
		page_count,
		flags,
	);
}

pub fn get_framebuffer_info() -> Option<&'static BgaInfo> {
	BGA_INFO.get()
}
