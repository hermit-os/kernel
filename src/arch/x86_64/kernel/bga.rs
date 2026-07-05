use core::sync::atomic::{AtomicU64, Ordering};

use memory_addresses::{PhysAddr, VirtAddr};
use pci_types::{Bar, CommandRegister};
use x86_64::instructions::port::Port;

use crate::arch::x86_64::mm::paging;
use crate::arch::x86_64::mm::paging::{BasePageSize, PageTableEntryFlags, PageTableEntryFlagsExt};
use crate::drivers::pci::PciDevice;
use crate::kernel::pci::PciConfigRegion;

static FRAMEBUFFER_PHYS: AtomicU64 = AtomicU64::new(0);

const VBE_DISPI_IOPORT_INDEX: u16 = 0x01ce;
const VBE_DISPI_IOPORT_DATA: u16 = 0x01cf;

const VBE_DISPI_INDEX_ID: u16 = 0;
const VBE_DISPI_INDEX_XRES: u16 = 1;
const VBE_DISPI_INDEX_YRES: u16 = 2;
const VBE_DISPI_INDEX_BPP: u16 = 3;
const VBE_DISPI_INDEX_ENABLE: u16 = 4;

const VBE_DISPI_DISABLED: u16 = 0x00;
const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;
const VBE_DISPI_ID5: u16 = 0xb0c5;

pub fn init_device(adapter: &PciDevice<PciConfigRegion>) {
	//To Do: Detect Resolution automatically
	let width: u16 = 640;
	let height: u16 = 400;
	let bpp: u16 = 32;

	unsafe {
		let mut index_port: Port<u16> = Port::new(VBE_DISPI_IOPORT_INDEX);
		let mut data_port: Port<u16> = Port::new(VBE_DISPI_IOPORT_DATA);

		index_port.write(VBE_DISPI_INDEX_ID);
		let bga_version = data_port.read();

		if bga_version != VBE_DISPI_ID5 {
			error!("Unsupported BGA version: {bga_version:#06x}");
			return;
		}

		index_port.write(VBE_DISPI_INDEX_ENABLE);
		data_port.write(VBE_DISPI_DISABLED);

		index_port.write(VBE_DISPI_INDEX_XRES);
		data_port.write(width);

		index_port.write(VBE_DISPI_INDEX_YRES);
		data_port.write(height);

		index_port.write(VBE_DISPI_INDEX_BPP);
		data_port.write(bpp);

		index_port.write(VBE_DISPI_INDEX_ENABLE);
		data_port.write(VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED);
	}

	adapter.set_command(CommandRegister::MEMORY_ENABLE);

	let (phys_addr, size) = match adapter.get_bar(0) {
		Some(Bar::Memory32 { address, size, .. }) => (u64::from(address), size as usize),
		Some(Bar::Memory64 { address, size, .. }) => (address, size as usize),
		_ => return,
	};

	FRAMEBUFFER_PHYS.store(phys_addr, Ordering::Release);

	assert!(
		size % 4096 == 0,
		"Framebuffer size must be a multiple of 4096 bytes"
	);
	let page_count = size / 4096;

	let mut flags = PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	flags.insert(PageTableEntryFlags::USER_ACCESSIBLE);
	paging::map::<BasePageSize>(
		VirtAddr::new(phys_addr),
		PhysAddr::new(phys_addr),
		page_count,
		flags,
	);
}

pub fn get_framebuffer_address() -> u64 {
	FRAMEBUFFER_PHYS.load(Ordering::Acquire)
}
