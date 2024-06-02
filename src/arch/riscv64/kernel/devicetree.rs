#[cfg(all(feature = "tcp", not(feature = "pci")))]
use core::ptr::NonNull;

use fdt::Fdt;
#[cfg(all(feature = "tcp", not(feature = "pci")))]
use virtio_spec::mmio::{DeviceRegisterVolatileFieldAccess, DeviceRegisters};
#[cfg(all(feature = "tcp", not(feature = "pci")))]
use volatile::VolatileRef;

#[cfg(feature = "gem-net")]
use crate::arch::mm::VirtAddr;
use crate::arch::riscv64::kernel::get_dtb_ptr;
use crate::arch::riscv64::kernel::interrupts::init_plic;
#[cfg(all(feature = "tcp", not(feature = "pci")))]
use crate::arch::riscv64::kernel::mmio::MmioDriver;
use crate::arch::riscv64::mm::{paging, PhysAddr};
#[cfg(feature = "gem-net")]
use crate::drivers::net::gem;
#[cfg(all(feature = "tcp", not(feature = "pci"), not(feature = "gem-net")))]
use crate::drivers::virtio::transport::mmio::{self as mmio_virtio, VirtioDriver};
#[cfg(all(feature = "tcp", not(feature = "pci")))]
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
	if !get_dtb_ptr().is_null() {
		unsafe {
			let fdt = Fdt::from_ptr(get_dtb_ptr()).expect("FDT is invalid");

			let model = fdt
				.find_node("/")
				.unwrap()
				.property("compatible")
				.expect("compatible not found in FDT")
				.as_str()
				.unwrap();

			if model.contains("riscv-virtio") {
				PLATFORM_MODEL = Model::Virt;
			} else if model.contains("sifive,hifive-unmatched-a00")
				|| model.contains("sifive,hifive-unleashed-a00")
				|| model.contains("sifive,fu740")
				|| model.contains("sifive,fu540")
			{
				PLATFORM_MODEL = Model::Fux40;
			} else {
				warn!("Unknown platform, guessing PLIC context 1");
				PLATFORM_MODEL = Model::Unknown;
			}
			info!("Model: {}", model);
		}
	}
}

/// Inits drivers based on the device tree
/// This function should only be called once
pub fn init_drivers() {
	// TODO: Implement devicetree correctly
	if !get_dtb_ptr().is_null() {
		unsafe {
			debug!("Init drivers using devicetree");
			let fdt = Fdt::from_ptr(get_dtb_ptr()).expect("FDT is invalid");

			// Init PLIC first
			if let Some(plic_node) = fdt.find_compatible(&["sifive,plic-1.0.0"]) {
				debug!("Found interrupt controller");
				let plic_region = plic_node
					.reg()
					.expect("Reg property for PLIC not found in FDT")
					.next()
					.unwrap();

				debug!(
					"Init PLIC at {:p}, size: {:x}",
					plic_region.starting_address,
					plic_region.size.unwrap()
				);
				paging::identity_map::<paging::HugePageSize>(
					PhysAddr(plic_region.starting_address as u64),
					PhysAddr(
						(plic_region.starting_address as usize + plic_region.size.unwrap() - 1)
							as u64,
					),
				);

				// TODO: Determine correct context via devicetree and allow more than one context
				match PLATFORM_MODEL {
					Model::Virt => init_plic(plic_region.starting_address as usize, 1),
					Model::Unknown => init_plic(plic_region.starting_address as usize, 1),
					Model::Fux40 => init_plic(plic_region.starting_address as usize, 2),
				}
			}

			// Init GEM
			#[cfg(all(feature = "tcp", feature = "gem-net"))]
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
				debug!("Local MAC address: {:x?}", mac);
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

				debug!(
					"Init GEM at {:p}, irq: {}, phy_addr: {}",
					gem_region.starting_address, irq, phy_addr
				);
				paging::identity_map::<paging::HugePageSize>(
					PhysAddr(gem_region.starting_address as u64),
					PhysAddr(
						(gem_region.starting_address as usize + gem_region.size.unwrap() - 1)
							as u64,
					),
				);
				match gem::init_device(
					VirtAddr(gem_region.starting_address as u64),
					irq.try_into().unwrap(),
					phy_addr,
					<[u8; 6]>::try_from(mac).expect("MAC with invalid length"),
				) {
					Ok(drv) => register_driver(MmioDriver::GEMNet(
						hermit_sync::InterruptSpinMutex::new(drv),
					)),
					Err(err) => error!("Could not initialize GEM driver: {err}"),
				}
			}

			// Init virtio-mmio
			#[cfg(all(feature = "tcp", not(feature = "pci")))]
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

				debug!(
					"Init virtio_mmio at {:p}, irq: {}",
					virtio_region.starting_address, irq
				);
				paging::identity_map::<paging::HugePageSize>(
					PhysAddr(virtio_region.starting_address as u64),
					PhysAddr(
						(virtio_region.starting_address as usize + virtio_region.size.unwrap() - 1)
							as u64,
					),
				);

				// Verify the first register value to find out if this is really an MMIO magic-value.
				let ptr = virtio_region.starting_address as *mut DeviceRegisters;
				let mmio = VolatileRef::new(NonNull::new(ptr).unwrap());

				let magic = mmio.as_ptr().magic_value().read().to_ne();
				let version = mmio.as_ptr().version().read().to_ne();

				const MMIO_MAGIC_VALUE: u32 = 0x74726976;
				if magic != MMIO_MAGIC_VALUE {
					error!("It's not a MMIO-device at {mmio:p}");
				}

				if version != 2 {
					warn!("Found a leagacy device, which isn't supported");
				} else {
					// We found a MMIO-device (whose 512-bit address in this structure).
					trace!("Found a MMIO-device at {mmio:p}");

					// Verify the device-ID to find the network card
					let id = mmio.as_ptr().device_id().read();

					if id != virtio_spec::Id::Net {
						debug!("It's not a network card at {mmio:p}");
					} else {
						info!("Found network card at {mmio:p}");

						// crate::arch::mm::physicalmem::reserve(
						// 	PhysAddr::from(current_address.align_down(BasePageSize::SIZE as usize)),
						// 	BasePageSize::SIZE as usize,
						// );

						#[cfg(all(feature = "tcp", not(feature = "gem-net")))]
						if let Ok(VirtioDriver::Network(drv)) =
							mmio_virtio::init_device(mmio, irq.try_into().unwrap())
						{
							register_driver(MmioDriver::VirtioNet(
								hermit_sync::InterruptSpinMutex::new(drv),
							))
						}
					}
				}
			}
		}
	}
}
