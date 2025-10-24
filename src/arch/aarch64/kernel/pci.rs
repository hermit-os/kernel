use arm_gic::{IntId, Trigger};
use bit_field::BitField;
use fdt::Fdt;
use fdt::node::FdtNode;
use free_list::PageLayout;
use memory_addresses::arch::aarch64::{PhysAddr, VirtAddr};
use pci_types::{
	Bar, CommandRegister, ConfigRegionAccess, InterruptLine, InterruptPin, MAX_BARS, PciAddress,
	PciHeader,
};

use crate::arch::aarch64::kernel::interrupts::GIC;
use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::drivers::pci::{PCI_DEVICES, PciDevice};
use crate::mm::{PageAlloc, PageRangeAllocator};
use crate::{core_id, env};

const PCI_MAX_DEVICE_NUMBER: u8 = 32;
const PCI_MAX_FUNCTION_NUMBER: u8 = 8;

#[derive(Debug, Copy, Clone)]
pub(crate) struct PciConfigRegion(VirtAddr);

impl PciConfigRegion {
	pub const fn new(addr: VirtAddr) -> Self {
		assert!(
			addr.as_u64() & 0x0fff_ffff == 0,
			"Unaligned PCI Config Space"
		);
		Self(addr)
	}

	#[inline]
	fn addr_from_offset(&self, pci_addr: PciAddress, offset: u16) -> usize {
		assert!(offset & 0xf000 == 0, "Invalid offset");
		((u64::from(pci_addr.bus()) << 20)
			| (u64::from(pci_addr.device()) << 15)
			| (u64::from(pci_addr.function()) << 12)
			| (u64::from(offset) & 0xfff)
			| self.0.as_u64()) as usize
	}
}

impl ConfigRegionAccess for PciConfigRegion {
	#[inline]
	unsafe fn read(&self, pci_addr: PciAddress, offset: u16) -> u32 {
		let ptr = core::ptr::with_exposed_provenance(self.addr_from_offset(pci_addr, offset));
		unsafe { u32::from_le(core::ptr::read_volatile(ptr)) }
	}

	#[inline]
	unsafe fn write(&self, pci_addr: PciAddress, offset: u16, value: u32) {
		let ptr = core::ptr::with_exposed_provenance_mut(self.addr_from_offset(pci_addr, offset));
		unsafe {
			core::ptr::write_volatile(ptr, value.to_le());
		}
	}
}

/// Try to find regions for the device registers
#[allow(unused_assignments)]
fn detect_pci_regions(pci_node: FdtNode<'_, '_>) -> (u64, u64, u64) {
	let mut io_start: u64 = 0;
	let mut mem32_start: u64 = 0;
	let mut mem64_start: u64 = 0;

	let mut residual_slice = pci_node.property("ranges").unwrap().value;
	let mut value_slice;
	while !residual_slice.is_empty() {
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let high = u32::from_be_bytes(value_slice.try_into().unwrap());
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let _mid = u32::from_be_bytes(value_slice.try_into().unwrap());
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let _low = u32::from_be_bytes(value_slice.try_into().unwrap());

		match high.get_bits(24..=25) {
			0b00 => debug!("Configuration space"),
			0b01 => {
				debug!("IO space");
				if io_start != 0 {
					warn!("Found already IO space");
				}

				(value_slice, residual_slice) =
					residual_slice.split_at(core::mem::size_of::<u64>());
				io_start = u64::from_be_bytes(value_slice.try_into().unwrap());
			}
			0b10 => {
				let prefetchable = high.get_bit(30);
				debug!("32 bit memory space: prefetchable {prefetchable}");
				if mem32_start != 0 {
					warn!("Found already 32 bit memory space");
				}

				(value_slice, residual_slice) =
					residual_slice.split_at(core::mem::size_of::<u64>());
				mem32_start = u64::from_be_bytes(value_slice.try_into().unwrap());
			}
			0b11 => {
				let prefetchable = high.get_bit(30);
				debug!("64 bit memory space: prefetchable {prefetchable}");
				if mem64_start != 0 {
					warn!("Found already 64 bit memory space");
				}

				(value_slice, residual_slice) =
					residual_slice.split_at(core::mem::size_of::<u64>());
				mem64_start = u64::from_be_bytes(value_slice.try_into().unwrap());
			}
			_ => panic!("Unknown space code"),
		}

		// currently, the size is ignores
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
		//let size = u64::from_be_bytes(value_slice.try_into().unwrap());
	}

	(io_start, mem32_start, mem64_start)
}

#[allow(unused_assignments)]
fn detect_interrupt(
	bus: u32,
	dev: u32,
	fdt: Fdt<'_>,
	pci_node: FdtNode<'_, '_>,
) -> Option<(InterruptPin, InterruptLine)> {
	let addr = (bus << 16) | (dev << 11);
	if addr == 0 {
		// assume PCI bridge => no configuration required
		return None;
	}

	let mut pin: u8 = 0;

	// let interrupt_parent = fdt.find_node("/").unwrap().interrupt_parent().unwrap()

	let cell_sizes = fdt.root().cell_sizes();

	// let interrupt_cells = fdt.find_node("/intc").unwrap().interrupt_cells().unwrap();

	let mut residual_slice = pci_node.property("interrupt-map").unwrap().value;
	let mut value_slice;
	while !residual_slice.is_empty() {
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let high = u32::from_be_bytes(value_slice.try_into().unwrap());
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let _mid = u32::from_be_bytes(value_slice.try_into().unwrap());
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let _low = u32::from_be_bytes(value_slice.try_into().unwrap());

		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		//let child_specifier = u32::from_be_bytes(value_slice.try_into().unwrap());

		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		//let parent = u32::from_be_bytes(value_slice.try_into().unwrap());

		for _i in 0..cell_sizes.address_cells {
			(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
			//let parent_address = u32::from_be_bytes(value_slice.try_into().unwrap());
		}

		// The 1st cell is the interrupt type; 0 for SPI interrupts, 1 for PPI
		// interrupts.
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let irq_type = u32::from_be_bytes(value_slice.try_into().unwrap());

		// The 2nd cell contains the interrupt number for the interrupt type.
		// SPI interrupts are in the range [0-987].  PPI interrupts are in the
		// range [0-15].
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let irq_number = u32::from_be_bytes(value_slice.try_into().unwrap());

		// The 3rd cell is the flags, encoded as follows:
		// bits[3:0] trigger type and level flags.
		//		1 = low-to-high edge triggered
		// 		2 = high-to-low edge triggered (invalid for SPIs)
		//		4 = active high level-sensitive
		//		8 = active low level-sensitive (invalid for SPIs).
		// bits[15:8] PPI interrupt cpu mask.  Each bit corresponds to each of
		// the 8 possible cpus attached to the GIC.  A bit set to '1' indicated
		// the interrupt is wired to that CPU.  Only valid for PPI interrupts.
		// Also note that the configurability of PPI interrupts is IMPLEMENTATION
		// DEFINED and as such not guaranteed to be present (most SoC available
		// in 2014 seem to ignore the setting of this flag and use the hardware
		// default value).
		(value_slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u32>());
		let irq_flags = u32::from_be_bytes(value_slice.try_into().unwrap());

		trace!("Interrupt type {irq_type:#x}, number {irq_number:#x} flags {irq_flags:#x}");

		if high.get_bits(0..24) == addr {
			pin += 1;
			if irq_type == 0 {
				// enable interrupt
				let irq_id = IntId::spi(irq_number);
				let mut gic = GIC.lock();
				let gic = gic.as_mut().unwrap();
				let cpu_id = core_id();
				gic.set_interrupt_priority(irq_id, Some(cpu_id as usize), 0x10);
				if irq_flags == 4 {
					gic.set_trigger(irq_id, Some(cpu_id as usize), Trigger::Level);
				} else if irq_flags == 2 {
					gic.set_trigger(irq_id, Some(cpu_id as usize), Trigger::Edge);
				} else {
					panic!("Invalid interrupt level!");
				}
				gic.enable_interrupt(irq_id, Some(cpu_id as usize), true);

				return Some((pin, irq_number.try_into().unwrap()));
			}
		}
	}

	None
}

pub fn init() {
	let fdt = env::fdt().unwrap();

	if let Some(pci_node) = fdt.find_compatible(&["pci-host-ecam-generic"]) {
		let reg = pci_node.reg().unwrap().next().unwrap();
		let addr = PhysAddr::from(reg.starting_address.addr());
		let size = u64::try_from(reg.size.unwrap()).unwrap();

		let layout = PageLayout::from_size_align(size.try_into().unwrap(), 0x1000_0000).unwrap();
		let page_range = PageAlloc::allocate(layout).unwrap();
		let pci_address = VirtAddr::from(page_range.start());
		info!(
			"Mapping PCI Enhanced Configuration Space interface to virtual address {pci_address:p} (size {size:#X})"
		);

		let mut flags = PageTableEntryFlags::empty();
		flags.device().writable().execute_disable();
		paging::map::<BasePageSize>(
			pci_address,
			addr,
			(size / BasePageSize::SIZE).try_into().unwrap(),
			flags,
		);

		let (mut io_start, mut mem32_start, mut mem64_start) = detect_pci_regions(pci_node);

		debug!("IO address space starts at{io_start:#X}");
		debug!("Memory32 address space starts at {mem32_start:#X}");
		debug!("Memory64 address space starts {mem64_start:#X}");
		assert!(io_start > 0);
		assert!(mem32_start > 0);
		assert!(mem64_start > 0);

		let max_bus_number = size
			/ (u64::from(PCI_MAX_DEVICE_NUMBER)
				* u64::from(PCI_MAX_FUNCTION_NUMBER)
				* BasePageSize::SIZE);
		info!("Scanning PCI Busses 0 to {}", max_bus_number - 1);

		let pci_config = PciConfigRegion::new(pci_address);
		for bus in 0..max_bus_number {
			for device in 0..PCI_MAX_DEVICE_NUMBER {
				let pci_address = PciAddress::new(0, bus.try_into().unwrap(), device, 0);
				let header = PciHeader::new(pci_address);

				let (device_id, vendor_id) = header.id(pci_config);
				if device_id != u16::MAX && vendor_id != u16::MAX {
					let dev = PciDevice::new(pci_address, pci_config);

					// Initializes BARs
					let mut cmd = CommandRegister::empty();
					let mut range_iter = 0..MAX_BARS;
					while let Some(i) = range_iter.next() {
						if let Some(bar) = dev.get_bar(i.try_into().unwrap()) {
							match bar {
								Bar::Io { .. } => {
									dev.set_bar(
										i.try_into().unwrap(),
										Bar::Io {
											port: io_start.try_into().unwrap(),
										},
									);
									io_start += 0x20;
									cmd |= CommandRegister::IO_ENABLE
										| CommandRegister::BUS_MASTER_ENABLE;
								}
								Bar::Memory32 {
									address: _,
									size,
									prefetchable,
								} => {
									dev.set_bar(
										i.try_into().unwrap(),
										Bar::Memory32 {
											address: mem32_start.try_into().unwrap(),
											size,
											prefetchable,
										},
									);
									mem32_start += u64::from(size);
									cmd |= CommandRegister::MEMORY_ENABLE
										| CommandRegister::BUS_MASTER_ENABLE;
								}
								Bar::Memory64 {
									address: _,
									size,
									prefetchable,
								} => {
									dev.set_bar(
										i.try_into().unwrap(),
										Bar::Memory64 {
											address: mem64_start,
											size,
											prefetchable,
										},
									);
									mem64_start += size;
									cmd |= CommandRegister::MEMORY_ENABLE
										| CommandRegister::BUS_MASTER_ENABLE;
									range_iter.next(); // Skip 32-bit bar that is part of the 64-bit bar
								}
							}
						}
					}
					dev.set_command(cmd);

					if let Some((pin, line)) =
						detect_interrupt(bus.try_into().unwrap(), device.into(), fdt, pci_node)
					{
						debug!(
							"Initialize interrupt pin {pin} and line {line} for device {device_id}"
						);
						dev.set_irq(pin, line);
					}

					PCI_DEVICES.with(|pci_devices| pci_devices.unwrap().push(dev));
				}
			}
		}

		return;
	} else if let Some(_pci_node) = fdt.find_compatible(&["pci-host-cam-generic"]) {
		warn!("Currently, pci-host-cam-generic isn't supported!");
	}

	warn!("Unable to find PCI bus");
}
