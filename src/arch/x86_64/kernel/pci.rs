// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::vec::Vec;
use arch::x86_64::kernel::pci_ids::{CLASSES, VENDORS};
use arch::x86_64::mm::paging;
use core::{fmt, u32, u8};
use synch::spinlock::Spinlock;
use x86::io::*;

const PCI_MAX_BUS_NUMBER: u8 = 32;
const PCI_MAX_DEVICE_NUMBER: u8 = 32;

const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;

const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;
const PCI_COMMAND_BUSMASTER: u32 = 1 << 2;

const PCI_ID_REGISTER: u32 = 0x00;
const PCI_COMMAND_REGISTER: u32 = 0x04;
const PCI_CLASS_REGISTER: u32 = 0x08;
const PCI_BAR0_REGISTER: u32 = 0x10;
const PCI_CAPABILITY_LIST_REGISTER: u32 = 0x34;
const PCI_INTERRUPT_REGISTER: u32 = 0x3C;

const PCI_STATUS_CAPABILITIES_LIST: u32 = 1 << 4;

pub const PCI_BASE_ADDRESS_IO_SPACE: u32 = 1 << 0;
pub const PCI_BASE_ADDRESS_64BIT: u32 = 1 << 2;
pub const PCI_BASE_ADDRESS_MASK: u32 = 0xFFFF_FFF0;

const PCI_CAP_ID_VNDR: u32 = 0x09;

/* Common configuration */
const VIRTIO_PCI_CAP_COMMON_CFG: u32 = 1;
/* Notifications */
const VIRTIO_PCI_CAP_NOTIFY_CFG: u32 = 2;
/* ISR Status */
const VIRTIO_PCI_CAP_ISR_CFG: u32 = 3;
/* Device specific configuration */
const VIRTIO_PCI_CAP_DEVICE_CFG: u32 = 4;
/* PCI configuration access */
const VIRTIO_PCI_CAP_PCI_CFG: u32 = 5;

const VIRTIO_F_RING_INDIRECT_DESC: u64 = 1 << 28;
const VIRTIO_F_RING_EVENT_IDX: u64 = 1 << 29;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_F_ACCESS_PLATFORM: u64 = 1 << 33;
const VIRTIO_F_RING_PACKED: u64 = 1 << 34;
const VIRTIO_F_IN_ORDER: u64 = 1 << 35;
const VIRTIO_F_ORDER_PLATFORM: u64 = 1 << 36;
const VIRTIO_F_SR_IOV: u64 = 1 << 37;
const VIRTIO_F_NOTIFICATION_DATA: u64 = 1 << 38;

static PCI_ADAPTERS: Spinlock<Vec<PciAdapter>> = Spinlock::new(Vec::new());

#[derive(Clone, Copy)]
pub struct PciAdapter {
	pub bus: u8,
	pub device: u8,
	pub vendor_id: u16,
	pub device_id: u16,
	pub class_id: u8,
	pub subclass_id: u8,
	pub programming_interface_id: u8,
	pub base_addresses: [u32; 6],
	pub base_sizes: [u32; 6],
	pub irq: u8,
}

impl PciAdapter {
	fn new(bus: u8, device: u8, vendor_id: u16, device_id: u16) -> Self {
		// TODO: check Header_Type for 0x00 (general purpose device), since irq is not defined otherwise!
		// TODO: warn if header_type specifies multifunciton, since we dont scan additional functions
		let class_ids = read_config(bus, device, PCI_CLASS_REGISTER);

		let mut base_addresses: [u32; 6] = [0; 6];
		let mut base_sizes: [u32; 6] = [0; 6];
		// TODO: this only works for I/O Space BARs! Verify that bit 0 is 1!
		for i in 0..6 {
			let register = PCI_BAR0_REGISTER + ((i as u32) << 2);
			let barword = read_config(bus, device, register);
			if barword & 1 == 0 {
				error!("Bar {} @{:x}:{:x} is memory mapped, but treated as IO mapped! this will cause errors later..", i, device_id, vendor_id);
			}
			base_addresses[i] = barword & 0xFFFF_FFFC;

			if base_addresses[i] > 0 {
				write_config(bus, device, register, u32::MAX);
				base_sizes[i] = !(read_config(bus, device, register) & PCI_BASE_ADDRESS_MASK) + 1;
				write_config(bus, device, register, base_addresses[i]);
			}
		}

		let interrupt_info = read_config(bus, device, PCI_INTERRUPT_REGISTER);

		Self {
			bus: bus,
			device: device,
			vendor_id: vendor_id,
			device_id: device_id,
			class_id: (class_ids >> 24) as u8,
			subclass_id: (class_ids >> 16) as u8,
			programming_interface_id: (class_ids >> 8) as u8,
			base_addresses: base_addresses,
			base_sizes: base_sizes,
			irq: interrupt_info as u8,
		}
	}

	pub fn make_bus_master(&self) {
		let mut command = read_config(self.bus, self.device, PCI_COMMAND_REGISTER);
		command |= PCI_COMMAND_BUSMASTER;
		write_config(self.bus, self.device, PCI_COMMAND_REGISTER, command);
	}
}

impl fmt::Display for PciAdapter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Look for the best matching class name in the PCI Database.
		let mut class_name = "Unknown Class";
		for ref c in CLASSES {
			if c.id == self.class_id {
				class_name = c.name;
				for ref sc in c.subclasses {
					if sc.id == self.subclass_id {
						class_name = sc.name;
						break;
					}
				}

				break;
			}
		}

		// Look for the vendor and device name in the PCI Database.
		let mut vendor_name = "Unknown Vendor";
		let mut device_name = "Unknown Device";
		for ref v in VENDORS {
			if v.id == self.vendor_id {
				vendor_name = v.name;
				for ref d in v.devices {
					if d.id == self.device_id {
						device_name = d.name;
						break;
					}
				}

				break;
			}
		}

		// Output detailed readable information about this device.
		write!(
			f,
			"{:02X}:{:02X} {} [{:02X}{:02X}]: {} {} [{:04X}:{:04X}]",
			self.bus,
			self.device,
			class_name,
			self.class_id,
			self.subclass_id,
			vendor_name,
			device_name,
			self.vendor_id,
			self.device_id
		)?;

		// If the devices uses an IRQ, output this one as well.
		if self.irq != 0 && self.irq != u8::MAX {
			write!(f, ", IRQ {}", self.irq)?;
		}

		write!(f, ", iobase ")?;
		for i in 0..self.base_addresses.len() {
			if self.base_addresses[i] > 0 {
				write!(
					f,
					"0x{:x} (size 0x{:x}) ",
					self.base_addresses[i], self.base_sizes[i]
				)?;
			}
		}

		Ok(())
	}
}

// TODO: use u8 for register (?)
fn read_config(bus: u8, device: u8, register: u32) -> u32 {
	let address =
		PCI_CONFIG_ADDRESS_ENABLE | u32::from(bus) << 16 | u32::from(device) << 11 | register;
	unsafe {
		outl(PCI_CONFIG_ADDRESS_PORT, address);
		inl(PCI_CONFIG_DATA_PORT)
	}
}

fn write_config(bus: u8, device: u8, register: u32, data: u32) {
	let address =
		PCI_CONFIG_ADDRESS_ENABLE | u32::from(bus) << 16 | u32::from(device) << 11 | register;
	unsafe {
		outl(PCI_CONFIG_ADDRESS_PORT, address);
		outl(PCI_CONFIG_DATA_PORT, data);
	}
}

pub fn get_adapter(vendor_id: u16, device_id: u16) -> Option<PciAdapter> {
	let adapters = PCI_ADAPTERS.lock();
	for adapter in adapters.iter() {
		if adapter.vendor_id == vendor_id && adapter.device_id == device_id {
			return Some(*adapter);
		}
	}

	None
}

struct virtq<'a> {
	num: u32,
	// The actial descriptors (16 bytes each)
	virtq_desc: &'a [virtq_desc],
	// A ring of available descriptor heads with free-running index
	avail: virtq_avail<'a>,
	// A ring of used descriptor heads with free-running index
	used: virtq_used<'a>,
}

// Virtqueue descriptors: 16 bytes.
// These can chain together via "next".
#[repr(C)]
struct virtq_desc {
	// Address (guest-physical)
	addr: u64,
	// Length
	len: u32,
	// The flags as indicated above
	flags: u16,
	// We chain unused descriptors via this, too
	next: u16,
}

#[repr(C)]
struct virtq_avail<'a> {
	flags: u16, // If VIRTIO_F_EVENT_IDX, set to 1 to maybe suppress interrupts
	idx: u16,
	ring: &'a [u16],
	// Only if VIRTIO_F_EVENT_IDX used_event: u16,
}

#[repr(C)]
struct virtq_used<'a> {
	flags: u16, // TODO: must init to 0
	ids: u16,
	ring: &'a [virtq_used_elem],
}

// u32 is used here for ids for padding reasons.
#[repr(C)]
struct virtq_used_elem {
	// Index of start of used descriptor chain.
	id: u32,
	// Total length of the descriptor chain which was written to.
	len: u32,
}

#[repr(C)]
struct virtio_pci_cap {
	cap_vndr: u8,     /* Generic PCI field: PCI_CAP_ID_VNDR */
	cap_next: u8,     /* Generic PCI field: next ptr. */
	cap_len: u8,      /* Generic PCI field: capability length */
	cfg_type: u8,     /* Identifies the structure. */
	bar: u8,          /* Where to find it. */
	padding: [u8; 3], /* Pad to full dword. */
	offset: u32,      /* Offset within bar. */
	length: u32,      /* Length of the structure, in bytes. */
}

#[repr(C)]
#[derive(Debug)]
struct virtio_pci_common_cfg {
	/* About the whole device. */
	device_feature_select: u32, /* read-write */
	device_feature: u32,        /* read-only for driver */
	driver_feature_select: u32, /* read-write */
	driver_feature: u32,        /* read-write */
	msix_config: u16,           /* read-write */
	num_queues: u16,            /* read-only for driver */
	device_status: u8,          /* read-write */
	config_generation: u8,      /* read-only for driver */

	/* About a specific virtqueue. */
	queue_select: u16,      /* read-write */
	queue_size: u16,        /* read-write, power of 2, or 0. */
	queue_msix_vector: u16, /* read-write */
	queue_enable: u16,      /* read-write */
	queue_notify_off: u16,  /* read-only for driver */
	queue_desc: u64,        /* read-write */
	queue_avail: u64,       /* read-write */
	queue_used: u64,        /* read-write */
}

#[repr(C)]
struct virtio_fs_config {
	/* Filesystem name (UTF-8, not NUL-terminated, padded with NULs) */
	tag: [u8; 36],
	/* Number of request queues */
	num_request_queues: u32,
}

impl fmt::Debug for virtio_fs_config {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"virtio_fs_config \{ tag: '{}', num_request_queues: {} \}",
			core::str::from_utf8(&self.tag).unwrap(),
			self.num_request_queues
		)
	}
}

/// Scans pci-capabilities for a virtio-capability of type virtiocaptype.
/// When found, maps it into memory and returns virtual address, else None
fn map_virtiocap(
	bus: u8,
	device: u8,
	adapter: &PciAdapter,
	caplist: u32,
	virtiocaptype: u32,
) -> Option<usize> {
	let mut nextcaplist = caplist;
	if nextcaplist < 0x40 {
		error!(
			"Caplist inside header! Offset: 0x{:x}, Aborting",
			nextcaplist
		);
		return None;
	}

	// Debug dump all
	/*for x in (0..255).step_by(4) {
			info!("{:02x}: {:08x}", x, read_config(bus, device, x));
	}*/

	// Loop through capabilities until vendor (virtio) defined one is found
	let virtiocapoffset = loop {
		if nextcaplist == 0 || nextcaplist < 0x40 {
			error!("Next caplist invalid, and still not found the wanted virtio cap, aborting!");
			return None;
		}
		let captypeword = read_config(bus, device, nextcaplist);
		info!(
			"Read cap at offset 0x{:x}: captype 0x{:x}",
			nextcaplist, captypeword
		);
		let captype = captypeword & 0xFF; // pci cap type
		if captype == PCI_CAP_ID_VNDR {
			// we are vendor defined, with virtio vendor --> we can check for virtio cap type
			info!("found vendor, virtio type: {}", (captypeword >> 24) & 0xFF);
			if (captypeword >> 24) & 0xFF == virtiocaptype {
				break nextcaplist;
			}
		}
		nextcaplist = (captypeword >> 8) & 0xFF; // pci cap next ptr
	};
	// Since we have verified caplistoffset to be virtio_pci_cap common config, read fields.
	// TODO: cleanup 'hacky' type conversions
	let bar: usize = (read_config(bus, device, virtiocapoffset + 4) & 0xFF) as usize; // get offset_of!(virtio_pci_cap, bar)
	let offset: usize = read_config(bus, device, virtiocapoffset + 8) as usize; // get offset_of!(virtio_pci_cap, offset)
	let length: usize = read_config(bus, device, virtiocapoffset + 12) as usize; // get offset_of!(virtio_pci_cap, length)
	info!(
		"Found virtio config bar as 0x{:x}, offset 0x{:x}, length 0x{:x}",
		bar, offset, length
	);

	if (adapter.base_sizes[bar] as usize) < offset + length {
		error!(
			"virtio config struct does not fit in bar! Aborting! 0x{:x} < 0x{:x}",
			adapter.base_sizes[bar],
			offset + length
		);
		return None;
	}

	// base_addresses from bar are IOBASE?
	// TODO: fix this hack. bar is assumed to be mem-mapped
	let barword = read_config(bus, device, PCI_BAR0_REGISTER + ((bar as u32) << 2));
	info!("Found bar{} as 0x{:x}", bar, barword);
	assert!(barword & 1 == 0, "Not an memory mapped bar!");

	let bartype = (barword >> 1) & 0b11;
	assert!(bartype == 2, "Not a 64 bit bar!");

	let prefetchable = (barword >> 3) & 1;
	assert!(prefetchable == 1, "Bar not prefetchable, but 64 bit!");

	let barwordhigh = read_config(bus, device, PCI_BAR0_REGISTER + (((bar + 1) as u32) << 2));
	//let barbase = barwordhigh << 33; // creates general protection fault... only when shifting by >=32 though..
	let barbase: usize = ((barwordhigh as usize) << 32) + (barword & 0xFFFF_FFF0) as usize;

	info!(
		"Mapping bar {} at 0x{:x} with length 0x{:x}",
		bar, barbase, length
	);
	// corrosponding setup in eg Qemu @ https://github.com/qemu/qemu/blob/master/hw/virtio/virtio-pci.c#L1590 (virtio_pci_device_plugged)
	let membase = barbase as *mut u8;
	let capbase = unsafe { membase.offset(offset as isize) as usize };
	let mut flags = paging::PageTableEntryFlags::empty();
	flags.device().writable().execute_disable();
	unsafe {
		// map 1 page (0x1000?) TODO: map "length"!
		// this maps membase physical to the same virtual address. Might conflict? TODO!
		// for virtio-fs we are "lucky" and each cap is exactly one page (contiguous, but we dont care, map each on its own)
		paging::map::<paging::BasePageSize>(capbase, capbase, 1, flags);
	}
	/*
	let mut slice = unsafe {
			core::slice::from_raw_parts_mut(membase.offset(offset as isize), length)
	};
	info!("{:?}", slice);
	*/
	Some(capbase)
}

pub fn init() {
	debug!("Scanning PCI Busses 0 to {}", PCI_MAX_BUS_NUMBER - 1);
	let mut adapters = PCI_ADAPTERS.lock();

	// HermitCore only uses PCI for network devices.
	// Therefore, multifunction devices as well as additional bridges are not scanned.
	// We also limit scanning to the first 32 buses.
	for bus in 0..PCI_MAX_BUS_NUMBER {
		'pciloop: for device in 0..PCI_MAX_DEVICE_NUMBER {
			let device_vendor_id = read_config(bus, device, PCI_ID_REGISTER);
			if device_vendor_id != u32::MAX {
				let device_id = (device_vendor_id >> 16) as u16;
				let vendor_id = device_vendor_id as u16;
				// 4.1.2
				if vendor_id == 0x1AF4 && device_id >= 0x1000 && device_id <= 0x107F {
					info!("Found virtio device with device id 0x{:x}", device_id);
					// TODO: 2.3.1: Loop until get_config_generation static, since it might change mid-read

					if device_id <= 0x103F {
						// Legacy device, skip
						continue;
					}
					let virtio_device_id = device_id - 0x1040;

					if virtio_device_id == 0x1a {
						info!("Found Virtio-FS device!");
					} else {
						// Not a virtio-fs device
						info!("Virtio device is NOT virtio-fs device, skipping!");
						continue;
					}

					// Scan capabilities to get common config, which we need to reset the device and get basic info.
					// also see https://elixir.bootlin.com/linux/latest/source/drivers/virtio/virtio_pci_modern.c#L581 (virtio_pci_modern_probe)
					// Read status register
					let status = read_config(bus, device, PCI_COMMAND_REGISTER) >> 16;

					// non-legacy virtio device always specifies capability list, so it can tell us in which bar we find the virtio-config-space
					if status & PCI_STATUS_CAPABILITIES_LIST == 0 {
						error!("Found virtio device without capability list. Likely legacy-device! Aborting.");
						continue;
					}

					let adapter = PciAdapter::new(bus, device, vendor_id, device_id);

					// Get pointer to capability list
					let caplist = read_config(bus, device, PCI_CAPABILITY_LIST_REGISTER) & 0xFF;

					// get common config mapped, cast to virtio_pci_common_cfg
					let common_cfg = match map_virtiocap(
						bus,
						device,
						&adapter,
						caplist,
						VIRTIO_PCI_CAP_COMMON_CFG,
					) {
						Some(cap_common_raw) => unsafe {
							&mut *(cap_common_raw as *mut virtio_pci_common_cfg)
						},
						None => {
							error!("Could not find VIRTIO_PCI_CAP_COMMON_CFG. Aborting!");
							continue;
						}
					};
					// get device config mapped, cast to virtio_fs_config
					let device_cfg = match map_virtiocap(
						bus,
						device,
						&adapter,
						caplist,
						VIRTIO_PCI_CAP_DEVICE_CFG,
					) {
						Some(cap_device_raw) => unsafe {
							&mut *(cap_device_raw as *mut virtio_fs_config)
						},
						None => {
							error!("Could not find VIRTIO_PCI_CAP_DEVICE_CFG. Aborting!");
							continue;
						}
					};
					// TODO: also load the other 3 header types (?).

					// 3.1 VirtIO Device Initialization
					// 1.Reset the device.
					//slice[0x12] = 0; // write 0 to device_status
					common_cfg.device_status = 0;

					// 2.Set the ACKNOWLEDGE status bit: the guest OS has notice the device.
					common_cfg.device_status |= 1;

					// 3.Set the DRIVER status bit: the guest OS knows how to drive the device.
					common_cfg.device_status |= 2;

					// 4.Read device feature bits, and write the subset of feature bits understood by the OS and driver to the device.
					//   During this step the driver MAY read (but MUST NOT write) the device-specific configuration fields to check
					//   that it can support the device before accepting it.
					{
						// Output debug info
						info!("Virtio common config struct: {:?}", common_cfg);
						info!("Virtio device config struct: {:?}", device_cfg);

						// Linux kernel reads 2x32 featurebits: https://elixir.bootlin.com/linux/latest/ident/vp_get_features
						common_cfg.device_feature_select = 0;
						let mut device_features: u64 = common_cfg.device_feature as u64;
						common_cfg.device_feature_select = 1;
						device_features |= (common_cfg.device_feature as u64) << 32;

						if device_features & VIRTIO_F_RING_INDIRECT_DESC != 0 {
							info!("Device offers feature VIRTIO_F_RING_INDIRECT_DESC, ignoring");
						}
						if device_features & VIRTIO_F_RING_EVENT_IDX != 0 {
							info!("Device offers feature VIRTIO_F_RING_EVENT_IDX, ignoring");
						}
						if device_features & VIRTIO_F_VERSION_1 != 0 {
							info!("Device offers feature VIRTIO_F_VERSION_1, accepting.");
							common_cfg.driver_feature_select = 1;
							common_cfg.driver_feature = (VIRTIO_F_VERSION_1 >> 32) as u32;
						}
						if device_features
							& !(VIRTIO_F_RING_INDIRECT_DESC
								| VIRTIO_F_RING_EVENT_IDX | VIRTIO_F_VERSION_1)
							!= 0
						{
							info!(
								"Device offers unknown feature bits: {:064b}.",
								device_features
							);
						}
						// There are no virtio-fs specific featurebits yet.
						// TODO: actually check features
						// currently provided features of virtio-fs:
						// 0000000000000000000000000000000100110000000000000000000000000000
						// only accept VIRTIO_F_VERSION_1 for now.
					}
					/*
					// on failure:
					common_cfg.device_status |= 128;
					continue;
					*/

					// 5.Set the FEATURES_OK status bit. The driver MUST NOT accept new feature bits after this step.
					common_cfg.device_status |= 8;

					// 6.Re-read device status to ensure the FEATURES_OK bit is still set:
					//   otherwise, the device does not support our subset of features and the device is unusable.
					if common_cfg.device_status & 8 == 0 {
						error!("Device unset FEATURES_OK, aborting!");
						continue;
					}

					// 7.Perform device-specific setup, including discovery of virtqueues for the device, optional per-bus setup,
					//   reading and possibly writing the device’s virtio configuration space, and population of virtqueues.
					{
						info!("Setting up virtqueues...");
						// see https://elixir.bootlin.com/linux/latest/ident/virtio_fs_setup_vqs for docs

						if device_cfg.num_request_queues == 0 {
							error!("0 request queues requested from device. Aborting!");
							continue;
						}
						// 1 highprio queue, and n normal request queues
						let vqnum = device_cfg.num_request_queues + 1;

						// alloc memory for vqnum queues.

						// 1.Write the virtqueue index (first queue is 0) to queue_select.
						// 2.Read the virtqueue size from queue_size. This controls how big the virtqueue is (see 2.4 Virtqueues).
						//   If this field is 0, the virtqueue does not exist.
						// 3.Optionally, select a smaller virtqueue size and write it to queue_size.
						// 4.Allocate and zero Descriptor Table, Available and Used rings for the virtqueue in contiguous physical memory.
						// 5.Optionally, if MSI-X capability is present and enabled on the device, select a vector to use to
						//   request interrupts triggered by virtqueue events. Write the MSI-X Table entry number corresponding to this
						//   vector into queue_msix_vector. Read queue_msix_vector:
						//   on success, previously written value is returned; on failure, NO_VECTOR value is returned.

						// The driver notifies the device by writing the 16-bit virtqueue index of this virtqueue to the Queue Notify address.
						// See 4.1.4.4 for how to calculate this address

						// Tell device about the physical addresses of our queue structs:
						common_cfg.queue_desc = 0;
						common_cfg.queue_avail = 0;
						common_cfg.queue_used = 0;
						common_cfg.queue_enable = 1;
					};

					// 8.Set the DRIVER_OK status bit. At this point the device is “live”.
					common_cfg.device_status |= 4;

					// Setup virtio-fs (5.11 in virtio spec @ https://stefanha.github.io/virtio/virtio-fs.html#x1-41500011)
					// 5.11.5 Device Initialization
					// On initialization the driver first discovers the device’s virtqueues.
					// The FUSE session is started by sending a FUSE_INIT request as defined by the FUSE protocol on one request virtqueue.
					// All virtqueues provide access to the same FUSE session and therefore only one FUSE_INIT request is required
					// regardless of the number of available virtqueues.

					// 5.11.6 Device Operation
					// TODO: send a simple getdents as test

					// Save adapter in list.
					adapters.push(adapter);
					continue;
				}

				adapters.push(PciAdapter::new(bus, device, vendor_id, device_id));
			}
		}
	}
}

pub fn print_information() {
	infoheader!(" PCI BUS INFORMATION ");

	let adapters = PCI_ADAPTERS.lock();
	for adapter in adapters.iter() {
		info!("{}", adapter);
	}

	infofooter!();
}
