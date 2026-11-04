use alloc::vec::Vec;
use core::ptr;
use core::ptr::NonNull;

use align_address::Align;
use free_list::PageRange;
#[cfg(any(
	feature = "virtio-fs",
	feature = "virtio-rng",
	feature = "virtio-vsock",
))]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::without_interrupts;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::arch::mm::paging;
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::drivers::InterruptHandlerMap;
#[cfg(feature = "virtio-fs")]
use crate::drivers::fs::VirtioFsDriver;
#[cfg(feature = "virtio-net")]
use crate::drivers::net::virtio::VirtioNetDriver;
#[cfg(feature = "virtio-rng")]
use crate::drivers::rng::VirtioRngDriver;
use crate::drivers::virtio::transport::mmio as mmio_virtio;
#[cfg(any(
	feature = "virtio-console",
	feature = "virtio-fs",
	feature = "virtio-net",
	feature = "virtio-rng",
	feature = "virtio-vsock",
))]
use crate::drivers::virtio::transport::mmio::VirtioDriver;
#[cfg(feature = "virtio-vsock")]
use crate::drivers::vsock::VirtioVsockDriver;
use crate::env::{self, VirtioMmioDevice};
#[cfg(any(feature = "rtl8139", feature = "virtio-net"))]
use crate::executor::device::NETWORK_DEVICE;
use crate::init_cell::InitCell;
use crate::mm::{FrameAlloc, PageRangeAllocator};

static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

#[allow(clippy::enum_variant_names, clippy::large_enum_variant)]
#[non_exhaustive]
pub(crate) enum MmioDriver {
	#[cfg(feature = "virtio-fs")]
	VirtioFs(InterruptTicketMutex<VirtioFsDriver>),
	#[cfg(feature = "virtio-rng")]
	VirtioRng(InterruptTicketMutex<VirtioRngDriver>),
	#[cfg(feature = "virtio-vsock")]
	VirtioVsock(InterruptTicketMutex<VirtioVsockDriver>),
}

impl MmioDriver {
	#[cfg(feature = "virtio-fs")]
	fn get_filesystem_driver(&self) -> Option<&InterruptTicketMutex<VirtioFsDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioFs(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(feature = "virtio-rng")]
	fn get_rng_driver(&self) -> Option<&InterruptTicketMutex<VirtioRngDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioRng(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(feature = "virtio-vsock")]
	fn get_vsock_driver(&self) -> Option<&InterruptTicketMutex<VirtioVsockDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioVsock(drv) => Some(drv),
			_ => None,
		}
	}
}

unsafe fn check_ptr(ptr: *mut u8) -> Option<VolatileRef<'static, DeviceRegisters>> {
	// Verify the first register value to find out if this is really an MMIO magic-value.
	let mmio = unsafe { VolatileRef::new(NonNull::new(ptr.cast::<DeviceRegisters>()).unwrap()) };

	let magic = mmio.as_ptr().magic_value().read();
	let version = mmio.as_ptr().version().read().to_ne();

	if magic != virtio::mmio::MAGIC_VALUE {
		trace!("It's not a MMIO-device at {mmio:p}");
		return None;
	}

	if version != 2 {
		trace!("Found a legacy device, which isn't supported");
		return None;
	}

	let id = mmio.as_ptr().device_id().read();

	if id == virtio::Id::Reserved {
		return None;
	}

	info!("Found Virtio {id:?} device: {mmio:p}");
	Some(mmio)
}

fn detect_device(phys_addr: usize) -> Option<VolatileRef<'static, DeviceRegisters>> {
	trace!("Trying to detect MMIO device at {phys_addr:#x}...");

	paging::identity_map::<BasePageSize>(phys_addr.align_down(BasePageSize::SIZE as usize).into());
	let ptr = ptr::with_exposed_provenance_mut::<u8>(phys_addr);

	let mmio = unsafe { check_ptr(ptr) }?;

	if cfg!(debug_assertions) {
		let len = usize::try_from(BasePageSize::SIZE).unwrap();
		let start = phys_addr.align_down(len);
		let frame_range = PageRange::from_start_len(start, len).unwrap();

		FrameAlloc::allocate_at(frame_range).unwrap_err();
	}

	Some(mmio)
}

fn check_linux_args(
	devices: &'static [VirtioMmioDevice],
) -> impl Iterator<Item = (VolatileRef<'static, DeviceRegisters>, u8)> {
	devices.iter().filter_map(|device| {
		let phys_addr = usize::try_from(device.base).ok()?;
		detect_device(phys_addr).map(|mmio| (mmio, device.irq))
	})
}

/// Returns the virtio-mmio devices (`LNRO0005`) described by ACPI,
/// or `None` if there are no ACPI tables.
#[cfg(feature = "acpi")]
fn acpi_devices() -> Option<Vec<(VolatileRef<'static, DeviceRegisters>, u8)>> {
	use acpi::aml::resource::{MemoryRangeDescriptor, Resource};

	let devices = crate::acpi::find_devices("LNRO0005")?
		.into_iter()
		.filter_map(|resources| {
			let base = resources.iter().find_map(|resource| match resource {
				Resource::MemoryRange(MemoryRangeDescriptor::FixedLocation {
					base_address,
					..
				}) => Some(*base_address),
				_ => None,
			})?;
			let irq = resources.iter().find_map(|resource| match resource {
				Resource::Irq(irq) => Some(irq.irq),
				_ => None,
			})?;
			let irq = u8::try_from(irq).ok()?;
			detect_device(usize::try_from(base).unwrap()).map(|mmio| (mmio, irq))
		})
		.collect();
	Some(devices)
}

#[cfg(not(feature = "acpi"))]
fn acpi_devices() -> Option<Vec<(VolatileRef<'static, DeviceRegisters>, u8)>> {
	None
}

fn guess_device() -> impl Iterator<Item = (VolatileRef<'static, DeviceRegisters>, u8)> {
	// From https://gitlab.com/qemu-project/qemu/-/blob/v10.2.2/include/hw/i386/microvm.h#L53.
	const VIRTIO_MMIO_BASE: usize = 0xfeb0_0000;
	// The layout of QEMU's microvm without ACPI, from
	// https://gitlab.com/qemu-project/qemu/-/blob/v10.2.2/hw/i386/microvm.c#L188-204.
	// With ACPI, the devices are found through the DSDT.
	const VIRTIO_IRQ_BASE: u8 = 5;
	const VIRTIO_NUM_TRANSPORTS: u8 = 8;

	(0..VIRTIO_NUM_TRANSPORTS).flat_map(move |i| {
		detect_device(VIRTIO_MMIO_BASE + usize::from(i) * 512)
			.map(|mmio| (mmio, VIRTIO_IRQ_BASE + i))
	})
}

#[cfg(any(
	feature = "virtio-fs",
	feature = "virtio-rng",
	feature = "virtio-vsock",
))]
pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(feature = "virtio-net")]
pub(crate) type NetworkDevice = VirtioNetDriver;

#[cfg(feature = "virtio-fs")]
pub(crate) fn get_filesystem_driver() -> Option<&'static InterruptTicketMutex<VirtioFsDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_filesystem_driver())
}

#[cfg(feature = "virtio-rng")]
pub(crate) fn get_rng_driver() -> Option<&'static InterruptTicketMutex<VirtioRngDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_rng_driver())
}

#[cfg(feature = "virtio-vsock")]
pub(crate) fn get_vsock_driver() -> Option<&'static InterruptTicketMutex<VirtioVsockDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_vsock_driver())
}

fn register_mmio(
	mmio: VolatileRef<'static, DeviceRegisters>,
	irq: u8,
	handlers: &mut InterruptHandlerMap,
) {
	match mmio_virtio::init_device(mmio, irq, handlers) {
		#[cfg(feature = "virtio-console")]
		Ok(VirtioDriver::Console(drv)) => {
			crate::console::switch_to_virtio(*drv);
		}
		#[cfg(feature = "virtio-fs")]
		Ok(VirtioDriver::Fs(drv)) => {
			register_driver(MmioDriver::VirtioFs(InterruptTicketMutex::new(*drv)));
		}
		#[cfg(feature = "virtio-net")]
		Ok(VirtioDriver::Net(drv)) => {
			*NETWORK_DEVICE.lock() = Some(*drv);
		}
		#[cfg(feature = "virtio-rng")]
		Ok(VirtioDriver::Rng(drv)) => {
			register_driver(MmioDriver::VirtioRng(InterruptTicketMutex::new(*drv)));
		}
		#[cfg(feature = "virtio-vsock")]
		Ok(VirtioDriver::Vsock(drv)) => {
			register_driver(MmioDriver::VirtioVsock(InterruptTicketMutex::new(*drv)));
		}
		Err(err) => error!("Could not initialize virtio-mmio device: {err}"),
	}
}

pub(crate) fn init_drivers(handlers: &mut InterruptHandlerMap) {
	without_interrupts(|| {
		let linux_mmio = env::mmio();

		if linux_mmio.is_empty() {
			let devices = acpi_devices().unwrap_or_else(|| guess_device().collect());
			for (mmio, irq) in devices {
				register_mmio(mmio, irq, handlers);
			}
		} else {
			for (mmio, irq) in check_linux_args(linux_mmio) {
				register_mmio(mmio, irq, handlers);
			}
		}

		MMIO_DRIVERS.finalize();
	});
}
