#[cfg(not(feature = "riscv-plic"))]
use alloc::collections::BTreeMap;
#[cfg(not(feature = "riscv-plic"))]
use alloc::vec::Vec;
#[cfg(all(feature = "virtio", not(feature = "pci")))]
use core::ptr::NonNull;

use memory_addresses::PhysAddr;
#[cfg(all(feature = "gem-net", not(feature = "pci")))]
use memory_addresses::VirtAddr;
#[cfg(not(feature = "riscv-plic"))]
use riscv::interrupt::Interrupt;
#[cfg(all(feature = "virtio", not(feature = "pci")))]
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
#[cfg(all(feature = "virtio", not(feature = "pci")))]
use volatile::VolatileRef;

use crate::arch::kernel::interrupts::EXTERNAL_INTERRUPT_CONTROLLER;
#[cfg(feature = "riscv-plic")]
use crate::arch::kernel::interrupts::init_plic;
#[cfg(not(feature = "riscv-plic"))]
use crate::arch::kernel::interrupts::{init_aplic, init_interrupt_files};
#[cfg(all(
	any(
		feature = "virtio-fs",
		feature = "virtio-rng",
		feature = "virtio-vsock",
	),
	not(feature = "pci"),
))]
use crate::arch::kernel::mmio::MmioDriver;
#[cfg(all(
	any(
		feature = "virtio-fs",
		feature = "virtio-rng",
		feature = "virtio-vsock",
	),
	not(feature = "pci")
))]
use crate::arch::kernel::mmio::register_driver;
#[cfg(all(any(feature = "virtio", feature = "gem-net"), not(feature = "pci")))]
use crate::arch::mm::paging::{self, PageSize};
use crate::drivers::InterruptHandlerMap;
#[cfg(all(feature = "gem-net", not(feature = "pci")))]
use crate::drivers::net::gem;
#[cfg(all(feature = "virtio", not(feature = "pci")))]
use crate::drivers::virtio::transport::mmio as mmio_virtio;
#[cfg(all(
	any(
		feature = "virtio-console",
		feature = "virtio-fs",
		feature = "virtio-net",
		feature = "virtio-rng",
		feature = "virtio-vsock",
	),
	not(feature = "pci"),
))]
use crate::drivers::virtio::transport::mmio::VirtioDriver;
use crate::env::{self, FdtStartInfo};
#[cfg(all(any(feature = "gem-net", feature = "virtio-net"), not(feature = "pci")))]
use crate::executor::device::NETWORK_DEVICE;
#[cfg(all(feature = "virtio", not(feature = "pci")))]
use crate::mm::PageRangeAllocator;

#[cfg(feature = "riscv-plic")]
enum Model {
	Fux40,
	Virt,
	Unknown,
}

pub enum InterruptType {
	/// Default or unspecified type
	None = 0,
	/// Low to high edge sensitive type enabled
	LowToHighEdge = 1,
	/// Active low level sensitive type enabled
	ActiveLowLevel = 2,
	/// Active high level sensitive type enabled
	ActiveHighLevel = 4,
	/// High to low edge sensitive type enabled
	HighToLowEdge = 8,
}
impl From<u32> for InterruptType {
	fn from(value: u32) -> Self {
		match value {
			0 => Self::None,
			1 => Self::LowToHighEdge,
			2 => Self::ActiveLowLevel,
			4 => Self::ActiveHighLevel,
			8 => Self::HighToLowEdge,
			_ => panic!("invalid InterruptType bits"),
		}
	}
}

/// Inits variables based on the device tree
/// This function should only be called once
pub fn init_interrupt_controller() {
	let Some(fdt) = env::start_info().fdt() else {
		return;
	};

	#[cfg(not(feature = "riscv-plic"))]
	if let Some(imsic_node) = find_imsic(&fdt) {
		let imsic_region = imsic_node
			.reg()
			.expect("Reg property for imsic not found in FDT")
			.next()
			.unwrap();
		let addr = PhysAddr::from(imsic_region.starting_address.addr());
		let size = imsic_region.size.unwrap();

		let interrupt_cells = 1;
		let mut num_harts = 0;

		// Build a mapping from interrupt-controller phandle to hart-id
		let mut intc_to_hart = BTreeMap::new();
		for cpu_node in fdt.find_node("/cpus").unwrap().children() {
			if !cpu_node
				.compatible()
				.is_some_and(|c| c.all().any(|x| x == "riscv"))
			{
				continue;
			}

			// Assumes cpu has only one child node which is the interrupt-controller
			let intc_node = cpu_node
				.children()
				.next()
				.expect("No child node found for cpu node in FDT");
			assert!(
				intc_node
					.compatible()
					.is_some_and(|c| c.all().any(|x| x == "riscv,cpu-intc")),
				"Child node of cpu node is not compatible with riscv,cpu-intc"
			);
			assert!(
				intc_node
					.interrupt_cells()
					.is_some_and(|c| c == interrupt_cells)
			);

			let intc_phandle = intc_node.property("phandle").unwrap().as_usize().unwrap();
			let hart_id = cpu_node.property("reg").unwrap().as_usize().unwrap();

			intc_to_hart.insert(intc_phandle, hart_id);

			num_harts += 1;
		}

		let mut num_interrupt_files = 0;
		let interrupts_extended = imsic_node.property("interrupts-extended").unwrap().value;
		let size_per_entry = (1 + interrupt_cells) * size_of::<u32>();

		// Build a mapping from hart-id to index of the interrupt file region
		let mut interrupt_file_indices: Vec<usize> = Vec::new();
		interrupt_file_indices.resize_with(interrupts_extended.len() / size_per_entry, || 0);
		for (index, entry) in interrupts_extended.chunks_exact(size_per_entry).enumerate() {
			let irq_type = u32::from_be_bytes(entry[4..8].try_into().unwrap());
			if irq_type != Interrupt::SupervisorExternal as u32 {
				continue;
			}

			let intc_phandle = u32::from_be_bytes(entry[0..4].try_into().unwrap());
			let hart_id = intc_to_hart
				.get(&(intc_phandle as usize))
				.expect("No cpu node found for interrupt-controller phandle in FDT");

			interrupt_file_indices[*hart_id] = index;

			num_interrupt_files += 1;
		}
		assert!(
			num_interrupt_files == num_harts,
			"Number of interrupt files does not match number of harts in FDT. AMP is not supported."
		);

		debug!("Found IMSIC at {addr:p}, size: {size:#x}, num_harts: {num_harts}");
		init_interrupt_files(addr, size, interrupt_file_indices);
	}

	#[cfg(not(feature = "riscv-plic"))]
	if let Some(aplic_node) = find_aplic(&fdt) {
		let aplic_region = aplic_node
			.reg()
			.expect("Reg property for APLIC not found in FDT")
			.next()
			.unwrap();
		let addr = PhysAddr::from(aplic_region.starting_address.addr());
		let size = aplic_region.size.unwrap();
		let msi_delivery = aplic_node.property("msi-parent").is_some();

		debug!("Found APLIC at {addr:p}, size: {size:#x}, msi_delivery: {msi_delivery:?}");
		init_aplic(addr, size, msi_delivery);
	}

	#[cfg(feature = "riscv-plic")]
	if let Some(plic_node) = fdt.find_compatible(&["sifive,plic-1.0.0"]) {
		debug!("Found external interrupt controller");
		let plic_region = plic_node
			.reg()
			.expect("Reg property for PLIC not found in FDT")
			.next()
			.unwrap();

		let plic_region_start = PhysAddr::from(plic_region.starting_address.addr());
		let plic_region_size = plic_region.size.unwrap();
		debug!("Init PLIC at {plic_region_start:p}, size: {plic_region_size:x}");

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
		info!("Model: {model}");

		// TODO: Determine correct context via devicetree and allow more than one context
		let context = match platform_model {
			Model::Virt | Model::Unknown => 1,
			Model::Fux40 => 2,
		};
		init_plic(plic_region_start, plic_region_size, context);
	}

	if EXTERNAL_INTERRUPT_CONTROLLER.lock().is_none() {
		warn!("No external interrupt controller found");
	}
}

#[cfg(not(feature = "riscv-plic"))]
pub fn msi_supported_vectors() -> Option<usize> {
	let fdt = env::start_info().fdt()?;
	let imsic_node = find_imsic(&fdt)?;

	imsic_node.property("riscv,num-ids")?.as_usize()
}

#[cfg(not(feature = "riscv-plic"))]
fn find_imsic<'a>(fdt: &'a fdt::Fdt<'_>) -> Option<fdt::node::FdtNode<'a, 'a>> {
	let mut node = fdt.find_compatible(&["riscv,imsics"])?;

	// Different interrupts domains, including m-mode domains, show up as different nodes.
	// We expect a hierarchy of one m-mode domain and one s-mode domain as described in
	// 'The RISC-V Advanced Interrupt Architecture', Version 1, Figure 4.2
	if node.property("status").and_then(|p| p.as_str()) == Some("disabled") {
		let phandle = node.property("riscv,children")?.as_usize()?;
		node = fdt.find_phandle(phandle as u32)?;

		// Ensure the S-mode domain is actually enabled
		assert!(
			node.property("status").and_then(|p| p.as_str()) != Some("disabled"),
			"Referenced s-mode interrupt domain is not enabled in FDT"
		);
	}

	Some(node)
}

#[cfg(not(feature = "riscv-plic"))]
fn find_aplic<'a>(fdt: &'a fdt::Fdt<'_>) -> Option<fdt::node::FdtNode<'a, 'a>> {
	let mut node = fdt.find_compatible(&["riscv,aplic"])?;

	// Different interrupts domains, including m-mode domains, show up as different nodes.
	// We expect a hierarchy of one m-mode domain and one s-mode domain as described in
	// 'The RISC-V Advanced Interrupt Architecture', Version 1, Figure 4.2
	if node.property("status").and_then(|p| p.as_str()) == Some("disabled") {
		let phandle = node.property("riscv,children")?.as_usize()?;
		node = fdt.find_phandle(phandle as u32)?;

		// Ensure the S-mode domain is actually enabled
		assert!(
			node.property("status").and_then(|p| p.as_str()) != Some("disabled"),
			"Referenced s-mode interrupt domain is not enabled in FDT"
		);
	}

	Some(node)
}

#[cfg_attr(
	any(not(any(feature = "gem-net", feature = "virtio")), feature = "pci"),
	expect(unused_variables)
)]
/// Inits drivers based on the device tree
/// This function should only be called once
pub fn init_drivers(handlers: &mut InterruptHandlerMap) {
	// TODO: Implement devicetree correctly
	if let Some(fdt) = env::start_info().fdt() {
		debug!("Init drivers using devicetree");

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
			let parent_interrupt_cells = gem_node
				.interrupt_parent()
				.expect("interrupt-parent node for virtio mmio not found in FDT")
				.interrupt_cells()
				.expect("#interrupt-cells property for virtio mmio missing or invalid");
			let (irq_number, source_mode) = match parent_interrupt_cells {
				1 => (irq as u32, 0),
				2 => ((irq >> 32) as u32, irq as u32),
				_ => {
					panic!("Unsupported #interrupt-cells value: {parent_interrupt_cells}");
				}
			};

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

			let gem_region_start = PhysAddr::from(gem_region.starting_address.expose_provenance());
			debug!("Init GEM at {gem_region_start:p}, irq: {irq}, phy_addr: {phy_addr}");
			assert!(
				gem_region.size.unwrap() < usize::try_from(paging::HugePageSize::SIZE).unwrap()
			);
			paging::identity_map::<paging::HugePageSize>(gem_region_start);
			match gem::init_device(
				VirtAddr::new(gem_region_start.as_u64()),
				irq_number.try_into().unwrap(),
				phy_addr,
				<[u8; 6]>::try_from(mac).expect("MAC with invalid length"),
				handlers,
			) {
				Ok(drv) => {
					EXTERNAL_INTERRUPT_CONTROLLER
						.lock()
						.as_mut()
						.unwrap()
						.set_interrupt_source_mode(
							irq_number.try_into().unwrap(),
							source_mode.into(),
						);
					*NETWORK_DEVICE.lock() = Some(drv);
				}
				Err(err) => error!("Could not initialize GEM driver: {err}"),
			}
		}

		// Init virtio-mmio
		#[cfg(all(feature = "virtio", not(feature = "pci")))]
		for virtio_node in fdt.all_nodes() {
			use crate::drivers::error::DriverError;
			use crate::drivers::virtio::error::VirtioError;

			let is_virtio_mmio = virtio_node
				.compatible()
				.is_some_and(|c| c.all().any(|x| x == "virtio,mmio"));
			if !is_virtio_mmio {
				continue;
			}
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
			let parent_interrupt_cells = virtio_node
				.interrupt_parent()
				.expect("interrupt-parent node for virtio mmio not found in FDT")
				.interrupt_cells()
				.expect("#interrupt-cells property for virtio mmio missing or invalid");
			let (irq_number, source_mode) = match parent_interrupt_cells {
				1 => (irq as u32, 0),
				2 => ((irq >> 32) as u32, irq as u32),
				_ => {
					panic!("Unsupported #interrupt-cells value: {parent_interrupt_cells}");
				}
			};

			let virtio_region_start =
				PhysAddr::from(virtio_region.starting_address.expose_provenance());

			debug!("Init virtio_mmio at {virtio_region_start:p}, irq: {irq}");
			assert!(
				virtio_region.size.unwrap() < usize::try_from(paging::HugePageSize::SIZE).unwrap()
			);
			paging::identity_map::<paging::HugePageSize>(virtio_region_start);

			// Verify the first register value to find out if this is really an MMIO magic-value.
			let ptr = virtio_region.starting_address as *mut DeviceRegisters;
			let mmio = unsafe { VolatileRef::new(NonNull::new(ptr).unwrap()) };

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

				use crate::mm::FrameAlloc;

				let start = virtio_region.starting_address.addr();
				let len = virtio_region.size.unwrap();
				let frame_range = PageRange::from_start_len(start, len).unwrap();

				FrameAlloc::allocate_at(frame_range).unwrap_err();
			}

			debug!("Found virtio {id:?} at {mmio:p}");

			let drv = match mmio_virtio::init_device(mmio, irq_number.try_into().unwrap(), handlers)
			{
				Ok(drv) => drv,
				Err(DriverError::InitVirtioDevFail(VirtioError::DevNotSupported(0))) => {
					continue;
				}
				Err(err) => {
					error!("Could not initialize virtio-mmio device: {err}");
					continue;
				}
			};

			EXTERNAL_INTERRUPT_CONTROLLER
				.lock()
				.as_mut()
				.unwrap()
				.set_interrupt_source_mode(irq_number.try_into().unwrap(), source_mode.into());

			match drv {
				#[cfg(feature = "virtio-console")]
				VirtioDriver::Console(drv) => crate::console::switch_to_virtio(*drv),
				#[cfg(feature = "virtio-fs")]
				VirtioDriver::Fs(drv) => {
					register_driver(MmioDriver::VirtioFs(hermit_sync::InterruptSpinMutex::new(
						*drv,
					)));
				}
				#[cfg(feature = "virtio-net")]
				VirtioDriver::Net(drv) => {
					*NETWORK_DEVICE.lock() = Some(*drv);
				}
				#[cfg(feature = "virtio-rng")]
				VirtioDriver::Rng(drv) => {
					register_driver(MmioDriver::VirtioRng(hermit_sync::InterruptSpinMutex::new(
						*drv,
					)));
				}
				#[cfg(feature = "virtio-vsock")]
				VirtioDriver::Vsock(drv) => {
					register_driver(MmioDriver::VirtioVsock(
						hermit_sync::InterruptSpinMutex::new(*drv),
					));
				}
			}
		}
	}

	#[cfg(all(any(feature = "virtio", feature = "gem-net"), not(feature = "pci")))]
	super::mmio::MMIO_DRIVERS.finalize();
}
