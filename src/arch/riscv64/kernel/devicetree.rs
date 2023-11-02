use fdt::Fdt;

use crate::arch::riscv64::kernel::get_dtb_ptr;
use crate::arch::riscv64::kernel::interrupts::init_plic;
#[cfg(all(feature = "tcp", not(feature = "pci")))]
use crate::arch::riscv64::kernel::mmio::{self, MmioDriver};
use crate::arch::riscv64::mm::{paging, PhysAddr};
#[cfg(feature = "gem-net")]
use crate::drivers::net::gem;
#[cfg(all(feature = "tcp", not(feature = "gem-net")))]
use crate::drivers::virtio::transport::mmio as mmio_virtio;
#[cfg(all(feature = "tcp", not(feature = "gem-net")))]
use crate::drivers::virtio::transport::mmio::{DevId, MmioRegisterLayout, VirtioDriver};

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
				#[cfg(feature = "gem-net")]
				match gem::init_device(
					VirtAddr(gem_region.starting_address as u64),
					irq.try_into().unwrap(),
					phy_addr.into(),
					<[u8; 6]>::try_from(mac).expect("MAC with invalid length"),
				) {
					Ok(drv) => register_driver(MmioDriver::GEMNet(
						hermit_sync::InterruptSpinMutex::new(drv),
					)),
					Err(_) => (), // could have information on error
				}
			}

			// Init virtio-mmio
			#[cfg(all(feature = "tcp", not(feature = "gem-net")))]
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
				let mmio = &mut *(virtio_region.starting_address as *mut MmioRegisterLayout);

				let magic = mmio.get_magic_value();
				let version = mmio.get_version();

				const MMIO_MAGIC_VALUE: u32 = 0x74726976;
				if magic != MMIO_MAGIC_VALUE {
					error!("It's not a MMIO-device at {:#X}", mmio as *const _ as usize);
				}

				if version != 2 {
					warn!("Found a leagacy device, which isn't supported");
				} else {
					// We found a MMIO-device (whose 512-bit address in this structure).
					trace!("Found a MMIO-device at {:#X}", mmio as *const _ as usize);

					// Verify the device-ID to find the network card
					let id = mmio.get_device_id();

					if id != DevId::VIRTIO_DEV_ID_NET {
						debug!(
							"It's not a network card at {:#X}",
							mmio as *const _ as usize
						);
					} else {
						info!("Found network card at {:#X}", mmio as *const _ as usize);

						// crate::arch::mm::physicalmem::reserve(
						// 	PhysAddr::from(current_address.align_down(BasePageSize::SIZE as usize)),
						// 	BasePageSize::SIZE as usize,
						// );

						#[cfg(all(feature = "tcp", not(feature = "gem-net")))]
						if let Ok(VirtioDriver::Network(drv)) =
							mmio_virtio::init_device(mmio, irq.try_into().unwrap())
						{
							mmio::register_driver(MmioDriver::VirtioNet(
								hermit_sync::InterruptSpinMutex::new(drv),
							))
						}
					}
				}
			}
		}
	}
}
