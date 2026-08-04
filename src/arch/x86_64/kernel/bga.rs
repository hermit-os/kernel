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

use crate::arch::kernel::pci::PciConfigRegion;
use crate::arch::x86_64::mm::paging::{
	self, BasePageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::drivers::pci::PciDevice;

#[derive(Debug)]
pub struct BgaInfo {
	pub framebuffer: usize,
	pub width: u16,
	pub height: u16,
	pub bpp: u16,
}

static BGA_INFO: OnceCell<BgaInfo> = OnceCell::new();

const VBE_DISPI_IOPORT_INDEX: u16 = 0x01ce;
const VBE_DISPI_IOPORT_DATA: u16 = 0x01cf;

#[allow(dead_code)]
#[repr(u16)]
pub enum VbeDispiIndex {
	#[doc(alias = "VBE_DISPI_INDEX_ID")]
	Id = 0,
	#[doc(alias = "VBE_DISPI_INDEX_XRES")]
	Xres = 1,
	#[doc(alias = "VBE_DISPI_INDEX_YRES")]
	Yres = 2,
	#[doc(alias = "VBE_DISPI_INDEX_BPP")]
	Bpp = 3,
	#[doc(alias = "VBE_DISPI_INDEX_ENABLE")]
	Enable = 4,
	#[doc(alias = "VBE_DISPI_INDEX_BANK")]
	Bank = 5,
	#[doc(alias = "VBE_DISPI_INDEX_VIRT_WIDTH")]
	VirtWidth = 6,
	#[doc(alias = "VBE_DISPI_INDEX_VIRT_HEIGHT")]
	VirtHeight = 7,
	#[doc(alias = "VBE_DISPI_INDEX_X_OFFSET")]
	XOffset = 8,
	#[doc(alias = "VBE_DISPI_INDEX_Y_OFFSET")]
	YOffset = 9,
}

const VBE_DISPI_DISABLED: u16 = 0x00;
const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;

#[allow(dead_code)]
const VBE_DISPI_NOCLEARMEM: u16 = 0x80;

#[allow(dead_code)]
#[repr(u16)]
pub enum VbeDispiId {
	#[doc(alias = "VBE_DISPI_ID0")]
	Id0 = 0xb0c0,
	#[doc(alias = "VBE_DISPI_ID1")]
	Id1 = 0xb0c1,
	#[doc(alias = "VBE_DISPI_ID2")]
	Id2 = 0xb0c2,
	#[doc(alias = "VBE_DISPI_ID3")]
	Id3 = 0xb0c3,
	#[doc(alias = "VBE_DISPI_ID4")]
	Id4 = 0xb0c4,
	#[doc(alias = "VBE_DISPI_ID5")]
	Id5 = 0xb0c5,
}

struct BgaRegisters;

impl BgaRegisters {
	pub fn read(index: VbeDispiIndex) -> u16 {
		let mut index_port: PortWriteOnly<u16> = PortWriteOnly::new(VBE_DISPI_IOPORT_INDEX);
		let mut data_port: Port<u16> = Port::new(VBE_DISPI_IOPORT_DATA);
		unsafe {
			index_port.write(index as u16);
			data_port.read()
		}
	}

	pub fn write(index: VbeDispiIndex, value: u16) {
		let mut index_port: PortWriteOnly<u16> = PortWriteOnly::new(VBE_DISPI_IOPORT_INDEX);
		let mut data_port: Port<u16> = Port::new(VBE_DISPI_IOPORT_DATA);
		unsafe {
			index_port.write(index as u16);
			data_port.write(value);
		}
	}
}

pub fn init_device(adapter: &PciDevice<PciConfigRegion>) {
	//To Do: Add support for different resolutions and BPP values
	let width: u16 = 640;
	let height: u16 = 400;
	let bpp: u16 = 32;

	let bga_version = BgaRegisters::read(VbeDispiIndex::Id);

	if bga_version != VbeDispiId::Id5 as u16 {
		error!("Unsupported BGA version: {bga_version:#06x}");
		return;
	}

	BgaRegisters::write(VbeDispiIndex::Enable, VBE_DISPI_DISABLED);
	BgaRegisters::write(VbeDispiIndex::Xres, width);
	BgaRegisters::write(VbeDispiIndex::Yres, height);
	BgaRegisters::write(VbeDispiIndex::Bpp, bpp);
	BgaRegisters::write(
		VbeDispiIndex::Enable,
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
			framebuffer: phys_addr as usize,
			width,
			height,
			bpp,
		})
		.unwrap();

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
