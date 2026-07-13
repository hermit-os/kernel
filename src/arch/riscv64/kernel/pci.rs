use core::ptr;

use align_address::Align;
use bit_field::BitField;
use fdt::Fdt;
use fdt::node::FdtNode;
use free_list::PageLayout;
use memory_addresses::{PhysAddr, VirtAddr};
use pci_types::{
	Bar, CommandRegister, ConfigRegionAccess, EndpointHeader, InterruptLine, MAX_BARS, PciAddress,
	PciHeader,
};

use crate::arch::riscv64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::drivers::pci::{PCI_DEVICES, PciDevice};
use crate::env;
use crate::mm::{PageAlloc, PageRangeAllocator};

const PCI_MAX_DEVICE_NUMBER: u8 = 32;
const PCI_MAX_FUNCTION_NUMBER: u8 = 8;

#[derive(Debug, Copy, Clone)]
pub struct PciConfigRegion {
	start: VirtAddr,
	end: VirtAddr,
}

impl PciConfigRegion {
	pub fn new(start: VirtAddr, size: u64) -> Self {
		assert!(
			start.as_u64() & 0x0fff_ffff == 0,
			"Unaligned PCI Config Space"
		);
		Self {
			start,
			end: start + size,
		}
	}

	#[inline]
	fn addr_from_offset(&self, pci_addr: PciAddress, offset: u16) -> usize {
		assert!(offset & 0xf000 == 0, "Invalid offset");
		let addr = (u64::from(pci_addr.bus()) << 20)
			| (u64::from(pci_addr.device()) << 15)
			| (u64::from(pci_addr.function()) << 12)
			| (u64::from(offset) & 0xfff)
			| self.start.as_u64();
		assert!(
			addr >= self.start.as_u64() && addr < self.end.as_u64(),
			"Address out of bounds"
		);
		addr as usize
	}
}

impl ConfigRegionAccess for PciConfigRegion {
	#[inline]
	unsafe fn read(&self, pci_addr: PciAddress, offset: u16) -> u32 {
		let ptr: *const u32 = ptr::with_exposed_provenance(self.addr_from_offset(pci_addr, offset));
		unsafe { u32::from_le(ptr.read_volatile()) }
	}

	#[inline]
	unsafe fn write(&self, pci_addr: PciAddress, offset: u16, value: u32) {
		let ptr: *mut u32 =
			ptr::with_exposed_provenance_mut(self.addr_from_offset(pci_addr, offset));
		unsafe {
			ptr.write_volatile(value.to_le());
		}
	}
}

/// Map the PCI Enhanced Configuration Space interface to a virtual address
fn mmap_ecam(pci_node: FdtNode<'_, '_>) -> (VirtAddr, u64) {
	let reg = pci_node.reg().unwrap().next().unwrap();
	let addr = PhysAddr::from(reg.starting_address.addr());
	let size = u64::try_from(reg.size.unwrap()).unwrap();

	let layout = PageLayout::from_size_align(size.try_into().unwrap(), 0x1000_0000).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let ecam_address = VirtAddr::from(page_range.start());

	info!(
		"Mapping PCI Enhanced Configuration Space interface to virtual address {ecam_address:p} (size {size:#X})"
	);

	let mut flags = PageTableEntryFlags::empty();
	flags.device().normal().writable().execute_disable();
	paging::map::<BasePageSize>(
		ecam_address,
		addr,
		(size / BasePageSize::SIZE).try_into().unwrap(),
		flags,
	);

	(ecam_address, size)
}

// Reference: <https://www.devicetree.org/open-firmware/bindings/pci2_1.pdf> (Chapter 2.2.1.1)
const PCI_SPACE_CODE_CONFIG: u32 = 0b00;
const PCI_SPACE_CODE_IO: u32 = 0b01;
const PCI_SPACE_CODE_MEM32: u32 = 0b10;
const PCI_SPACE_CODE_MEM64: u32 = 0b11;

struct PciSpaceAllocator {
	io_start: u32,
	io_end: u32,
	mem32_start: u32,
	mem32_end: u32,
	mem64_start: u64,
	mem64_end: u64,
}

/// Allocates address space for PCI devices from the ranges property of the PCI node in the device tree
///
/// References:
/// - <https://github.com/devicetree-org/devicetree-specification/releases/download/v0.4/devicetree-specification-v0.4.pdf> (Chapter 2.3.8)
/// - PCI LOCAL BUS SPECIFICATION, REV. 3.0, Chapter 6.2.5.1. Address Maps
impl PciSpaceAllocator {
	pub fn new(pci_node: FdtNode<'_, '_>) -> Self {
		//TODO: Use to fdt::node::Node::ranges() when fdt 0.2 stable version is released
		let ranges = pci_node.property("ranges").unwrap().value;

		let cell_size = size_of::<u32>();
		let address_cells = pci_node.cell_sizes().address_cells;
		assert!(address_cells == 3, "Invalid address-cells for PCI node");
		let parent_address_cells = 2; // not easily accessible in fdt version 0.1
		let size_cells = pci_node.cell_sizes().size_cells;
		assert!(size_cells == 2, "Invalid size-cells for PCI node");
		let cells_per_entry = address_cells + parent_address_cells + size_cells;
		assert!(
			ranges.len() == 3 * cells_per_entry * cell_size,
			"Invalid ranges property length"
		);

		let mut io_start: u64 = 0;
		let mut io_end: u64 = 0;
		let mut mem32_start: u64 = 0;
		let mut mem32_end: u64 = 0;
		let mut mem64_start: u64 = 0;
		let mut mem64_end: u64 = 0;

		for entry in ranges.chunks_exact(cells_per_entry * cell_size) {
			let child_bus_address_high = u32::from_be_bytes(entry[0..4].try_into().unwrap());
			let parent_bus_address = u64::from_be_bytes(entry[12..20].try_into().unwrap());
			let size = u64::from_be_bytes(entry[20..28].try_into().unwrap());

			match child_bus_address_high.get_bits(24..=25) {
				PCI_SPACE_CODE_CONFIG => debug!("Configuration space"),
				PCI_SPACE_CODE_IO => {
					assert!(io_start == 0, "Found already IO space");
					io_start = parent_bus_address;
					io_end = io_start + size;
				}
				PCI_SPACE_CODE_MEM32 => {
					assert!(mem32_start == 0, "Found already 32 bit memory space");
					mem32_start = parent_bus_address;
					mem32_end = mem32_start + size;
				}
				PCI_SPACE_CODE_MEM64 => {
					assert!(mem64_start == 0, "Found already 64 bit memory space");
					mem64_start = parent_bus_address;
					mem64_end = mem64_start + size;
				}
				_ => panic!("Unknown space code"),
			}
		}

		assert!(io_start != 0, "IO space not found");
		assert!(mem32_start != 0, "32 bit memory space not found");
		assert!(mem64_start != 0, "64 bit memory space not found");

		Self {
			io_start: io_start.try_into().unwrap(),
			io_end: io_end.try_into().unwrap(),
			mem32_start: mem32_start.try_into().unwrap(),
			mem32_end: mem32_end.try_into().unwrap(),
			mem64_start,
			mem64_end,
		}
	}

	pub fn allocate_io(&mut self, size: u32) -> Option<u32> {
		assert!(
			size.get_bits(0..=1) == 0,
			"Bits 0..=1 are not usable. Minimum size is 16 Bytes"
		);
		assert!(
			(4..=256).contains(&size),
			"Size must be between 4 and 256 Bytes"
		);
		let addr = self.io_start.align_up(size);
		if addr + size <= self.io_end {
			self.io_start = addr + size;
			Some(addr)
		} else {
			None
		}
	}

	pub fn allocate_mem32(&mut self, size: u32) -> Option<u32> {
		assert!(
			size.get_bits(0..=3) == 0,
			"Bits 0..=3 are not usable. Minimum size is 16 Bytes"
		);
		let addr = self.mem32_start.align_up(size);
		if addr + size <= self.mem32_end {
			self.mem32_start = addr + size;
			Some(addr)
		} else {
			None
		}
	}

	pub fn allocate_mem64(&mut self, size: u64) -> Option<u64> {
		assert!(
			size.get_bits(0..=3) == 0,
			"Bits 0..=3 are not usable. Minimum size is 16 Bytes"
		);
		let addr = self.mem64_start.align_up(size);
		if addr + size <= self.mem64_end {
			self.mem64_start = addr + size;
			Some(addr)
		} else {
			None
		}
	}
}

/// Iterator for PCI Device Enumeration
///
/// Reference: PCI LOCAL BUS SPECIFICATION, REV. 3.0, Chapter 6
struct PciDeviceIterator<'a, T: ConfigRegionAccess> {
	access: &'a T,
	max_bus: u16,
	current_bus: u16,
	current_device: u8,
	current_function: u8,
	max_functions_for_current_device: u8,
}

impl<T: ConfigRegionAccess> PciDeviceIterator<'_, T> {
	pub fn new(access: &T, max_bus: u16) -> PciDeviceIterator<'_, T> {
		PciDeviceIterator {
			access,
			max_bus,
			current_bus: 0,
			current_device: 0,
			current_function: 0,
			max_functions_for_current_device: 1,
		}
	}

	fn advance_device(&mut self) {
		self.current_function = 0;
		self.max_functions_for_current_device = 1;
		self.current_device += 1;
		if self.current_device >= 32 {
			self.current_device = 0;
			self.current_bus += 1;
		}
	}
}

impl<T: ConfigRegionAccess + Copy> Iterator for PciDeviceIterator<'_, T> {
	type Item = PciAddress;

	fn next(&mut self) -> Option<Self::Item> {
		while self.current_bus < self.max_bus {
			let bus = self.current_bus as u8;
			let device = self.current_device;
			let function = self.current_function;

			let addr = PciAddress::new(0, bus, device, function);
			let header = PciHeader::new(addr);
			let (device_id, vendor_id) = header.id(self.access);

			if function == 0 {
				if device_id == u16::MAX || vendor_id == u16::MAX {
					self.advance_device();
					continue;
				}
				self.max_functions_for_current_device =
					if header.has_multiple_functions(self.access) {
						8
					} else {
						1
					};
			}

			self.current_function += 1;
			if self.current_function >= self.max_functions_for_current_device {
				self.advance_device();
			}

			if device_id != u16::MAX && vendor_id != u16::MAX {
				return Some(addr);
			}
		}
		None
	}
}

/// Detect interrupt line for a PCI device based on the device tree.
///
/// References:
/// - <https://github.com/devicetree-org/devicetree-specification/releases/download/v0.4/devicetree-specification-v0.4.pdf> (Chapter 2.4.4)
#[allow(unused_assignments)]
fn detect_interrupt_line(
	bus: u8,
	device: u8,
	function: u8,
	interrupt_pin: u8,
	fdt: Fdt<'_>,
	pci_node: FdtNode<'_, '_>,
) -> Option<InterruptLine> {
	if interrupt_pin == 0 {
		// Device does not use legacy line-based interrupts
		return None;
	}

	let address_cells = pci_node.cell_sizes().address_cells;
	assert!(address_cells == 3, "Invalid address-cells for PCI node");
	let interrupt_cells = pci_node.interrupt_cells().unwrap();
	assert!(interrupt_cells == 1, "Invalid interrupt-cells for PCI node");

	let interrupt_map_mask_raw = pci_node.property("interrupt-map-mask").unwrap().value;
	assert!(
		interrupt_map_mask_raw.len() == (address_cells + interrupt_cells) * size_of::<u32>(),
		"Invalid interrupt-map-mask property length"
	);
	let interrupt_map_mask = [
		u32::from_be_bytes(interrupt_map_mask_raw[0..4].try_into().unwrap()),
		u32::from_be_bytes(interrupt_map_mask_raw[4..8].try_into().unwrap()),
		u32::from_be_bytes(interrupt_map_mask_raw[8..12].try_into().unwrap()),
		u32::from_be_bytes(interrupt_map_mask_raw[12..16].try_into().unwrap()),
	];

	let mut residual_slice = pci_node.property("interrupt-map").unwrap().value;
	let mut value_slice;
	while !residual_slice.is_empty() {
		(value_slice, residual_slice) = residual_slice.split_at(size_of::<u32>());
		let _child_unit_address_high = u32::from_be_bytes(value_slice.try_into().unwrap());
		(value_slice, residual_slice) = residual_slice.split_at(size_of::<u32>());
		let _child_unit_address_mid = u32::from_be_bytes(value_slice.try_into().unwrap());
		(value_slice, residual_slice) = residual_slice.split_at(size_of::<u32>());
		let _child_unit_address_low = u32::from_be_bytes(value_slice.try_into().unwrap());

		(value_slice, residual_slice) = residual_slice.split_at(size_of::<u32>());
		let _child_interrupt_specifier = u32::from_be_bytes(value_slice.try_into().unwrap());

		(value_slice, residual_slice) = residual_slice.split_at(size_of::<u32>());
		let interrupt_parent_phandle = u32::from_be_bytes(value_slice.try_into().unwrap());
		let interrupt_parent_node = fdt.find_phandle(interrupt_parent_phandle).unwrap();
		assert!(interrupt_parent_node.cell_sizes().address_cells == 0);
		assert!(interrupt_parent_node.interrupt_cells().unwrap() == 1);

		(value_slice, residual_slice) = residual_slice.split_at(size_of::<u32>());
		let parent_interrupt_specifier = u32::from_be_bytes(value_slice.try_into().unwrap());

		let key_interrupt_map = [
			_child_unit_address_high & interrupt_map_mask[0],
			_child_unit_address_mid & interrupt_map_mask[1],
			_child_unit_address_low & interrupt_map_mask[2],
			_child_interrupt_specifier & interrupt_map_mask[3],
		];
		let key_device = [
			u32::from(bus) << 16 | u32::from(device) << 11 | u32::from(function) << 8,
			0x0,
			0x0,
			u32::from(interrupt_pin),
		];
		if key_interrupt_map == key_device {
			return Some(InterruptLine::try_from(parent_interrupt_specifier).unwrap());
		}
	}

	None
}

pub fn init() {
	let fdt = env::fdt().unwrap();

	if let Some(pci_node) = fdt.find_compatible(&["pci-host-ecam-generic"]) {
		let (ecam_address, ecam_size) = mmap_ecam(pci_node);
		let mut pci_space_allocator = PciSpaceAllocator::new(pci_node);

		let max_bus_number: u16 = (ecam_size
			/ (u64::from(PCI_MAX_DEVICE_NUMBER)
				* u64::from(PCI_MAX_FUNCTION_NUMBER)
				* BasePageSize::SIZE))
			.try_into()
			.unwrap();
		let pci_config = PciConfigRegion::new(ecam_address, ecam_size);
		let pci_device_enumerator = PciDeviceIterator::new(&pci_config, max_bus_number);

		for dev_addr in pci_device_enumerator {
			let dev = PciDevice::new(dev_addr, pci_config);

			/* Initialize BARs */
			let mut cmd = CommandRegister::empty();
			let mut range_iter = 0..MAX_BARS;
			while let Some(i) = range_iter.next() {
				let Some(bar) = dev.get_bar(i.try_into().unwrap()) else {
					continue;
				};

				match bar {
					Bar::Io { .. } => {
						dev.set_bar(
							i.try_into().unwrap(),
							Bar::Io {
								port: pci_space_allocator.allocate_io(0x20).unwrap(),
							},
						);
						cmd |= CommandRegister::IO_ENABLE | CommandRegister::BUS_MASTER_ENABLE;
					}
					Bar::Memory32 {
						address: _,
						size,
						prefetchable,
					} => {
						dev.set_bar(
							i.try_into().unwrap(),
							Bar::Memory32 {
								address: pci_space_allocator.allocate_mem32(size).unwrap(),
								size,
								prefetchable,
							},
						);
						cmd |= CommandRegister::MEMORY_ENABLE | CommandRegister::BUS_MASTER_ENABLE;
					}
					Bar::Memory64 {
						address: _,
						size,
						prefetchable,
					} => {
						dev.set_bar(
							i.try_into().unwrap(),
							Bar::Memory64 {
								address: pci_space_allocator.allocate_mem64(size).unwrap(),
								size,
								prefetchable,
							},
						);
						cmd |= CommandRegister::MEMORY_ENABLE | CommandRegister::BUS_MASTER_ENABLE;
						range_iter.next(); // Skip 32-bit bar that is part of the 64-bit bar
					}
				}
			}
			dev.set_command(cmd);

			/* Initialize Interrupts */
			let header = PciHeader::new(dev_addr);
			let endpoint = EndpointHeader::from_header(header, pci_config).unwrap();
			let interrupt_pin = endpoint.interrupt(pci_config).0;
			if let Some(interrupt_line) = detect_interrupt_line(
				dev_addr.bus(),
				dev_addr.device(),
				dev_addr.function(),
				interrupt_pin,
				fdt,
				pci_node,
			) {
				debug!(
					"Initialize interrupt pin {interrupt_pin} and line {interrupt_line} for device {dev}"
				);
				dev.set_irq(interrupt_pin, interrupt_line);
			}

			PCI_DEVICES.with(|pci_devices| pci_devices.unwrap().push(dev));
		}
	} else if let Some(_pci_node) = fdt.find_compatible(&["pci-host-cam-generic"]) {
		warn!("Currently, pci-host-cam-generic isn't supported!");
	} else {
		warn!("Unable to find PCI bus");
	}
}
