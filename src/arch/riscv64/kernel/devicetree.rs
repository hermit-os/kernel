#![allow(dead_code)]

#[cfg(all(
	any(feature = "virtio-net", feature = "virtio-console"),
	not(feature = "pci")
))]
use core::ptr::NonNull;

use memory_addresses::PhysAddr;
#[cfg(all(feature = "gem-net", not(feature = "pci")))]
use memory_addresses::VirtAddr;
#[cfg(all(
	any(feature = "virtio-net", feature = "virtio-console"),
	not(feature = "pci")
))]
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
#[cfg(all(
	any(feature = "virtio-net", feature = "virtio-console"),
	not(feature = "pci")
))]
use volatile::VolatileRef;

use crate::arch::riscv64::kernel::interrupts::init_plic;
#[cfg(all(feature = "virtio-console", not(feature = "pci")))]
use crate::arch::riscv64::kernel::mmio::MmioDriver;
use crate::arch::riscv64::mm::paging::{self, PageSize};
#[cfg(feature = "virtio-console")]
use crate::console::IoDevice;
#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioUART;
#[cfg(all(feature = "virtio-console", not(feature = "pci")))]
use crate::drivers::mmio::get_console_driver;
#[cfg(all(feature = "gem-net", not(feature = "pci")))]
use crate::drivers::net::gem;
#[cfg(all(feature = "virtio-console", feature = "pci"))]
use crate::drivers::pci::get_console_driver;
#[cfg(all(
	any(feature = "virtio-net", feature = "virtio-console"),
	not(feature = "pci")
))]
use crate::drivers::virtio::transport::mmio::{self as mmio_virtio, VirtioDriver};
use crate::env;
#[cfg(all(any(feature = "gem-net", feature = "virtio-net"), not(feature = "pci")))]
use crate::executor::device::NETWORK_DEVICE;
#[cfg(all(feature = "virtio-console", not(feature = "pci")))]
use crate::kernel::mmio::register_driver;

static mut PLATFORM_MODEL: Model = Model::Unknown;

enum Model {
	Fux40,
	Virt,
	Unknown,
}

/// Inits variables based on the device tree
/// This function should only be called once
pub fn init() {
	debug!("Init devicetree");
	if let Some(fdt) = env::fdt() {
		let model = fdt
			.find_node("/")
			.unwrap()
			.property("compatible")
			.expect("compatible not found in FDT")
			.as_str()
			.unwrap();

		let platform_model = if model.contains("riscv-virtio") {
			Model::Virt
		} else if model.contains("sifive,hifive-unmatched-a00")
			|| model.contains("sifive,hifive-unleashed-a00")
			|| model.contains("sifive,fu740")
			|| model.contains("sifive,fu540")
		{
			Model::Fux40
		} else {
			warn!("Unknown platform, guessing PLIC context 1");
			Model::Unknown
		};
		unsafe {
			PLATFORM_MODEL = platform_model;
		}
		info!("Model: {model}");
	}
}

/// Inits drivers based on the device tree
/// This function should only be called once
pub fn init_drivers() {
	// TODO: Implement devicetree correctly
	if let Some(fdt) = env::fdt() {
		debug!("Init drivers using devicetree");

		unsafe {
			// Init PLIC first
			if let Some(plic_node) = fdt.find_compatible(&["sifive,plic-1.0.0"]) {
				debug!("Found interrupt controller");
				let plic_region = plic_node
					.reg()
					.expect("Reg property for PLIC not found in FDT")
					.next()
					.unwrap();

				let plic_region_start = PhysAddr::new(plic_region.starting_address as u64);
				debug!(
					"Init PLIC at {:p}, size: {:x}",
					plic_region_start,
					plic_region.size.unwrap()
				);
				assert!(
					plic_region.size.unwrap()
						< usize::try_from(paging::HugePageSize::SIZE).unwrap()
				);

				paging::identity_map::<paging::HugePageSize>(plic_region_start);

				// TODO: Determine correct context via devicetree and allow more than one context
				match PLATFORM_MODEL {
					Model::Virt | Model::Unknown => {
						init_plic(plic_region.starting_address as usize, 1);
					}
					Model::Fux40 => init_plic(plic_region.starting_address as usize, 2),
				}
			}

			// Init GEM
			#[cfg(all(feature = "gem-net", not(feature = "pci")))]
			if let Some(gem_node) = fdt.find_compatible(&["sifive,fu540-c000-gem"]) {
				debug!("Found Ethernet controller");

				let gem_region = gem_node
					.reg()
					.expect("reg property for GEM not found in FDT")
					.next()
					.unwrap();
				let irq = gem_node
					.interrupts()
					.expect("interrupts property for GEM not found in FDT")
					.next()
					.unwrap();
				let mac = gem_node
					.property("local-mac-address")
					.expect("local-mac-address property for GEM not found in FDT")
					.value;
				debug!("Local MAC address: {mac:x?}");
				let mut phy_addr = u32::MAX;

				let phy_node = gem_node
					.children()
					.next()
					.expect("GEM node has no child node (i. e. ethernet-phy)");
				if phy_node.name.contains("ethernet-phy") {
					phy_addr = phy_node
						.property("reg")
						.expect("reg property for ethernet-phy not found in FDT")
						.as_usize()
						.unwrap() as u32;
				} else {
					warn!("Expected ethernet-phy node, found something else");
				}

				let gem_region_start = PhysAddr::new(gem_region.starting_address as u64);
				debug!("Init GEM at {gem_region_start:p}, irq: {irq}, phy_addr: {phy_addr}");
				assert!(
					gem_region.size.unwrap() < usize::try_from(paging::HugePageSize::SIZE).unwrap()
				);
				paging::identity_map::<paging::HugePageSize>(gem_region_start);
				match gem::init_device(
					VirtAddr::new(gem_region_start.as_u64()),
					irq.try_into().unwrap(),
					phy_addr,
					<[u8; 6]>::try_from(mac).expect("MAC with invalid length"),
				) {
					Ok(drv) => *NETWORK_DEVICE.lock() = Some(drv),
					Err(err) => error!("Could not initialize GEM driver: {err}"),
				}
			}

			// Init virtio-mmio
			#[cfg(all(
				any(feature = "virtio-net", feature = "virtio-console"),
				not(feature = "pci")
			))]
			if let Some(virtio_node) = fdt.find_compatible(&["virtio,mmio"]) {
				debug!("Found virtio mmio device");
				let virtio_region = virtio_node
					.reg()
					.expect("reg property for virtio mmio not found in FDT")
					.next()
					.unwrap();
				let irq = virtio_node
					.interrupts()
					.expect("interrupts property for virtio mmio not found in FDT")
					.next()
					.unwrap();

				let virtio_region_start = PhysAddr::new(virtio_region.starting_address as u64);

				debug!("Init virtio_mmio at {virtio_region_start:p}, irq: {irq}");
				assert!(
					virtio_region.size.unwrap()
						< usize::try_from(paging::HugePageSize::SIZE).unwrap()
				);
				paging::identity_map::<paging::HugePageSize>(virtio_region_start);

				// Verify the first register value to find out if this is really an MMIO magic-value.
				let ptr = virtio_region.starting_address as *mut DeviceRegisters;
				let mmio = VolatileRef::new(NonNull::new(ptr).unwrap());

				let magic = mmio.as_ptr().magic_value().read().to_ne();
				let version = mmio.as_ptr().version().read().to_ne();

				const MMIO_MAGIC_VALUE: u32 = 0x7472_6976;
				if magic != MMIO_MAGIC_VALUE {
					error!("It's not a MMIO-device at {mmio:p}");
				}

				if version != 2 {
					warn!("Found a legacy device, which isn't supported");
					return;
				}

				// We found a MMIO-device (whose 512-bit address in this structure).
				trace!("Found a MMIO-device at {mmio:p}");

				// Verify the device-ID to find the network card
				let id = mmio.as_ptr().device_id().read();

				if cfg!(debug_assertions) {
					use free_list::PageRange;

					use crate::mm::{FrameAlloc, PageRangeAllocator};

					let start = virtio_region.starting_address.addr();
					let len = virtio_region.size.unwrap();
					let frame_range = PageRange::from_start_len(start, len).unwrap();

					FrameAlloc::allocate_at(frame_range).unwrap_err();
				}

				debug!("Found virtio {id:?} at {mmio:p}");

				match mmio_virtio::init_device(mmio, irq.try_into().unwrap()) {
					#[cfg(feature = "virtio-console")]
					Ok(VirtioDriver::Console(drv)) => {
						register_driver(MmioDriver::VirtioConsole(
							hermit_sync::InterruptSpinMutex::new(*drv),
						));
					}
					#[cfg(feature = "virtio-net")]
					Ok(VirtioDriver::Net(drv)) => {
						*NETWORK_DEVICE.lock() = Some(*drv);
					}
					Err(err) => error!("Could not initialize virtio-mmio device: {err}"),
				}
			}
		}
	}

	#[cfg(all(
		any(
			feature = "virtio-net",
			feature = "virtio-console",
			feature = "gem-net"
		),
		not(feature = "pci"),
	))]
	super::mmio::MMIO_DRIVERS.finalize();

	#[cfg(feature = "virtio-console")]
	{
		if get_console_driver().is_some() {
			info!("Switch to virtio console");
			crate::console::CONSOLE
				.lock()
				.replace_device(IoDevice::Virtio(VirtioUART::new()));
		}
	}
}
