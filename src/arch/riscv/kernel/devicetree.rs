use crate::arch::riscv::kernel::get_dtb_ptr;
use crate::arch::riscv::kernel::irq::init_plic;
use crate::arch::riscv::kernel::mmio::*;
use crate::arch::riscv::mm::{paging, PhysAddr, VirtAddr};
use crate::drivers::virtio::transport::mmio as mmio_virtio;
use crate::drivers::virtio::transport::mmio::{DevId, MmioRegisterLayout, VirtioDriver};
use alloc::vec::Vec;
use core::convert::TryFrom;
use hermit_dtb::Dtb;

use crate::drivers::net::gem;
use crate::synch::spinlock::SpinlockIrqSave;

pub const MAGIC_VALUE: u32 = 0x74726976 as u32;
static mut PLATFORM_MODEL: Model = Model::UNKNOWN;

enum Model {
	FUX40,
	VIRT,
	UNKNOWN,
}

struct Gem {
	base: usize,
	size: usize,
	irq: u8,
	phy_addr: u8,
	mac: [u8; 6],
}

struct VirtioMMIO {
	base: usize,
	size: usize,
	irq: u32,
}

struct Plic {
	base: usize,
	size: usize,
}

enum Device {
	GEM(Gem),
	PLIC(Plic),
	VIRTIO_MMIO(VirtioMMIO),
}

static mut DEVICES_AVAILABLE: Vec<Device> = Vec::new();

/// Inits variables based on the device tree
/// This function should only be called once
pub fn init() {
	debug!("Init devicetree");
	if !get_dtb_ptr().is_null() {
		unsafe {
			let dtb = Dtb::from_raw(get_dtb_ptr()).expect("DTB is invalid");

			let model = core::str::from_utf8(
				dtb.get_property("/", "compatible")
					.expect("compatible not found in dtb"),
			)
			.unwrap();
			if model.contains("riscv-virtio") {
				PLATFORM_MODEL = Model::VIRT;
			} else if model.contains("sifive,hifive-unmatched-a00")
				|| model.contains("sifive,hifive-unleashed-a00")
				|| model.contains("sifive,fu740")
				|| model.contains("sifive,fu540")
			{
				PLATFORM_MODEL = Model::FUX40;
			} else {
				warn!("Unknown platform, guessing PLIC context 1");
				PLATFORM_MODEL = Model::UNKNOWN;
			}
			info!("Model: {}", model);
		}
	}
}

/// Inits drivers based on the device tree
/// This function should only be called once
pub fn init_drivers() {
	// TODO: Implement devicetree correctly
	unsafe {
		if !get_dtb_ptr().is_null() {
			debug!("Init drivers using devicetree");
			let dtb = Dtb::from_raw(get_dtb_ptr()).expect("DTB is invalid");
			walk_nodes(&dtb, "/", 0);
		}
		for i in 0..DEVICES_AVAILABLE.len() {
			match &DEVICES_AVAILABLE[i] {
				Device::GEM(gem) => {
					//TODO: Make sure that PLIC is initialized
					debug!(
						"Init GEM at {:x}, irq: {}, phy_addr: {}",
						gem.base, gem.irq, gem.phy_addr
					);
					paging::identity_map::<paging::HugePageSize>(
						PhysAddr(gem.base as u64),
						PhysAddr((gem.base + gem.size - 1) as u64),
					);
					match gem::init_device(
						VirtAddr(gem.base as u64),
						gem.irq.into(),
						gem.phy_addr.into(),
						gem.mac,
					) {
						Ok(drv) => register_driver(MmioDriver::GEMNet(SpinlockIrqSave::new(drv))),
						Err(_) => (), // could have an info which driver failed
					}
				}
				Device::VIRTIO_MMIO(dev) => {
					//TODO: Make sure that PLIC is initialized
					debug!("Init virtio_mmio at {:x}, irq: {}", dev.base, dev.irq);
					paging::identity_map::<paging::HugePageSize>(
						PhysAddr(dev.base as u64),
						PhysAddr((dev.base + dev.size - 1) as u64),
					);

					// Verify the first register value to find out if this is really an MMIO magic-value.
					let mmio = unsafe { &mut *(dev.base as *mut MmioRegisterLayout) };

					let magic = mmio.get_magic_value();
					let version = mmio.get_version();

					if magic != MAGIC_VALUE {
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
							// 	PhysAddr::from(align_down!(current_address, BasePageSize::SIZE)),
							// 	BasePageSize::SIZE,
							// );

							match mmio_virtio::init_device(mmio, dev.irq) {
								Ok(VirtioDriver::Network(drv)) => register_driver(
									MmioDriver::VirtioNet(SpinlockIrqSave::new(drv)),
								),
								Err(_) => (), // could have an info which driver failed
							}
						}
					}
				}
				Device::PLIC(plic) => {
					debug!("Init PLIC at {:x}, size: {:x}", plic.base, plic.size);
					paging::identity_map::<paging::HugePageSize>(
						PhysAddr(plic.base as u64),
						PhysAddr((plic.base + plic.size - 1) as u64),
					);

					//TODO: Determine correct context via devicetree and allow more than one context
					match PLATFORM_MODEL {
						Model::VIRT => init_plic(plic.base, 1),
						Model::UNKNOWN => init_plic(plic.base, 1),
						Model::FUX40 => init_plic(plic.base, 2),
					}
				}
			}
		}
		// loop {}
	}
}

fn walk_nodes<'a, 'b>(dtb: &Dtb<'a>, path: &'b str, level: usize) {
	// debug!("{}: Path: {}", level, path);
	for prop in dtb.enum_properties(path) {
		debug!(
			"{}Prop: {}: {:x?}",
			"\t".repeat(level),
			prop,
			dtb.get_property(path, prop)
		);
	}
	for node in dtb.enum_subnodes(path) {
		debug!("{}{}", "\t".repeat(level), node);
		if node.starts_with("ethernet@") {
			debug!("Found Ethernet controller");
			let path = &[path, node, "/"].concat();

			let compatible = core::str::from_utf8(
				dtb.get_property(path, "compatible")
					.expect("compatible property for ethernet not found in dtb"),
			)
			.unwrap();
			debug!("Compatible: {}", compatible);

			if compatible.contains("sifive,fu540-c000-gem") {
				let reg = dtb
					.get_property(path, "reg")
					.expect("Reg property for ethernet not found in dtb");
				let mut gem_size: u64 = 0;
				let mut gem_base: u64 = 0;
				for i in 8..16 {
					gem_size <<= 8;
					gem_size += reg[i] as u64;
				}
				for i in 0..8 {
					gem_base <<= 8;
					gem_base += reg[i] as u64;
				}

				let interrupts = dtb
					.get_property(path, "interrupts")
					.expect("interrupts property for ethernet not found in dtb");
				let irq: u8 = interrupts[3];

				let mac = dtb
					.get_property(path, "local-mac-address")
					.expect("local-mac-address property for ethernet not found in dtb");
				debug!("MAC: {:x?}", mac);

				let path = &[path, "ethernet-phy"].concat();
				debug!("{}", path);
				let phy = dtb
					.get_property(path, "reg")
					.expect("Reg property for ethernet-phy not found in dtb");
				let phy_addr: u8 = phy[3];

				unsafe {
					DEVICES_AVAILABLE.push(Device::GEM(Gem {
						base: gem_base as usize,
						size: gem_size as usize,
						irq: irq,
						phy_addr: phy_addr,
						mac: <[u8; 6]>::try_from(mac).expect("mac with invalid length"),
					}));
				}
			} else {
				warn!("The ethernet controller is not supported");
			}
		} else if node.starts_with("interrupt-controller@") || node.starts_with("plic@") {
			debug!("Found interrupt controller");
			let path = &[path, node, "/"].concat();

			let compatible = core::str::from_utf8(
				dtb.get_property(path, "compatible")
					.expect("compatible property for interrupt-controller not found in dtb"),
			)
			.unwrap();
			debug!("Compatible: {}", compatible);
			if compatible.contains("sifive,plic-1.0.0") {
				let reg = dtb
					.get_property(path, "reg")
					.expect("Reg property for plic not found in dtb");
				let mut plic_size: u64 = 0;
				let mut plic_base: u64 = 0;
				for i in 8..16 {
					plic_size <<= 8;
					plic_size += reg[i] as u64;
				}
				for i in 0..8 {
					plic_base <<= 8;
					plic_base += reg[i] as u64;
				}

				unsafe {
					//Insert before
					DEVICES_AVAILABLE.insert(
						0,
						Device::PLIC(Plic {
							base: plic_base as usize,
							size: plic_size as usize,
						}),
					);
				}
			} else {
				warn!("The interrupt controller is not supported");
			}
		} else if node.starts_with("virtio_mmio@") {
			debug!("Found virtio mmio device");
			let path = &[path, node, "/"].concat();

			let reg = dtb
				.get_property(path, "reg")
				.expect("Reg property for virtio mmio not found in dtb");
			let mut virtio_size: u64 = 0;
			let mut virtio_base: u64 = 0;
			for i in 8..16 {
				virtio_size <<= 8;
				virtio_size += reg[i] as u64;
			}
			for i in 0..8 {
				virtio_base <<= 8;
				virtio_base += reg[i] as u64;
			}

			let interrupts = dtb
				.get_property(path, "interrupts")
				.expect("interrupts property for virtio mmio not found in dtb");
			let irq: u8 = interrupts[3];

			unsafe {
				DEVICES_AVAILABLE.push(Device::VIRTIO_MMIO(VirtioMMIO {
					base: virtio_base as usize,
					size: virtio_size as usize,
					irq: irq as u32,
				}));
			}
		}
		walk_nodes(&dtb, &[path, node, "/"].concat(), level + 1);
	}
}
