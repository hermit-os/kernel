#[cfg(all(feature = "virtio", not(feature = "pci")))]
use core::ptr::NonNull;

use memory_addresses::PhysAddr;
#[cfg(all(feature = "gem-net", not(feature = "pci")))]
use memory_addresses::VirtAddr;
use riscv::interrupt::Interrupt;
#[cfg(all(feature = "virtio", not(feature = "pci")))]
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
#[cfg(all(feature = "virtio", not(feature = "pci")))]
use volatile::VolatileRef;

use crate::arch::kernel::interrupts::EXTERNAL_INTERRUPT_CONTROLLER;
#[cfg(not(feature = "riscv-plic"))]
use crate::arch::kernel::interrupts::init_aplic;
#[cfg(feature = "riscv-plic")]
use crate::arch::kernel::interrupts::init_plic;
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

pub fn init_interrupt_controller() {
	let Some(fdt) = env::start_info().fdt() else {
		return;
	};

	#[cfg(not(feature = "riscv-plic"))]
	if let Some(aplic_node) = find_aplic(&fdt) {
		let aplic_region = aplic_node
			.reg()
			.expect("Reg property for APLIC not found in FDT")
			.next()
			.unwrap();
		let addr = PhysAddr::from(aplic_region.starting_address.addr());
		let size = aplic_region.size.unwrap();

		let msi_delivery = false;

		// Route interrupts to the boot hart, which runs the async executor.
		let boot_hart_id = super::get_current_boot_id() as usize;
		let hart_index = external_interrupt_index(&fdt, aplic_node, boot_hart_id)
			.expect("No S-mode APLIC hart index found for boot hart in FDT");
		debug!(
			"Found APLIC at {addr:p}, size: {size:#x}, msi_delivery: {msi_delivery:?}, hart_index: {hart_index}"
		);
		init_aplic(addr, size, msi_delivery, hart_index);
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

		// Route interrupts to the boot hart, which runs the async executor
		let boot_hart_id = super::get_current_boot_id() as usize;
		let context = external_interrupt_index(&fdt, plic_node, boot_hart_id)
			.expect("No S-mode PLIC context found for boot hart in FDT");
		debug!("Using PLIC context {context} for hart {boot_hart_id}");
		init_plic(plic_region_start, plic_region_size, context);
	}

	if EXTERNAL_INTERRUPT_CONTROLLER.lock().is_none() {
		warn!("No external interrupt controller found");
	}
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

			let magic = mmio.as_ptr().magic_value().read();
			let version = mmio.as_ptr().version().read().to_ne();

			if magic != virtio::mmio::MAGIC_VALUE {
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

/// Returns the index of the entry in `node`'s `interrupts-extended` property that delivers
/// supervisor external interrupts to `hart_id`.
///
/// Each entry references the `riscv,cpu-intc` of a hart and the interrupt it raises there.
/// For a PLIC, the index is the context of the hart. For an APLIC in direct delivery mode
/// and for an IMSIC, the index is the hart index of the hart.
///
/// References:
/// <https://github.com/torvalds/linux/blob/60490ca6d54b6f0a00223a4fe59bb180bb1538bf/Documentation/devicetree/bindings/interrupt-controller/sifive%2Cplic-1.0.0.yaml#L62-L67>
/// <https://github.com/torvalds/linux/blob/60490ca6d54b6f0a00223a4fe59bb180bb1538bf/Documentation/devicetree/bindings/interrupt-controller/riscv%2Caplic.yaml#L42-L48>
/// <https://github.com/torvalds/linux/blob/60490ca6d54b6f0a00223a4fe59bb180bb1538bf/Documentation/devicetree/bindings/interrupt-controller/riscv%2Cimsics.yaml#L69-L76>
fn external_interrupt_index(
	fdt: &fdt::Fdt<'_>,
	node: fdt::node::FdtNode<'_, '_>,
	hart_id: usize,
) -> Option<u16> {
	let cpu_node = fdt
		.find_node("/cpus")?
		.children()
		.find(|node| node.property("reg").and_then(|reg| reg.as_usize()) == Some(hart_id))?;
	let intc_node = cpu_node.children().find(|node| {
		node.compatible()
			.is_some_and(|compatible| compatible.all().any(|c| c == "riscv,cpu-intc"))
	})?;
	let intc_phandle = u32::try_from(intc_node.property("phandle")?.as_usize()?).ok()?;

	assert_eq!(intc_node.interrupt_cells(), Some(1));
	let interrupts_extended = node.property("interrupts-extended")?.value;
	let (cells, _) = interrupts_extended.as_chunks::<{ size_of::<u32>() }>();
	let (entries, _) = cells.as_chunks::<2>();
	let index = entries.iter().position(|&[phandle, irq]| {
		u32::from_be_bytes(phandle) == intc_phandle
			&& u32::from_be_bytes(irq) == Interrupt::SupervisorExternal as u32
	})?;

	Some(u16::try_from(index).unwrap())
}
