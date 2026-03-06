use ahash::RandomState;
use hashbrown::HashMap;

#[cfg(feature = "virtio-console")]
pub(crate) use crate::arch::kernel::mmio::get_console_driver;
#[cfg(feature = "virtio-fs")]
pub(crate) use crate::arch::kernel::mmio::get_filesystem_driver;
#[cfg(feature = "virtio-vsock")]
pub(crate) use crate::arch::kernel::mmio::get_vsock_driver;
#[cfg(any(
	feature = "virtio-console",
	all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
	feature = "virtio-net",
	feature = "virtio-fs",
	feature = "virtio-vsock",
))]
use crate::drivers::Driver;
use crate::drivers::{InterruptHandlerQueue, InterruptLine};
#[cfg(any(
	all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
	feature = "virtio-net",
))]
use crate::executor::device::NETWORK_DEVICE;

pub(crate) fn get_interrupt_handlers() -> HashMap<InterruptLine, InterruptHandlerQueue, RandomState>
{
	#[allow(unused_mut)]
	let mut handlers: HashMap<InterruptLine, InterruptHandlerQueue, RandomState> =
		HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0));

	#[cfg(any(
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		feature = "virtio-net",
	))]
	if let Some(device) = NETWORK_DEVICE.lock().as_ref() {
		handlers
			.entry(device.get_interrupt_number())
			.or_default()
			.push_back(crate::executor::network::network_handler);
	}

	#[cfg(feature = "virtio-console")]
	if let Some(drv) = get_console_driver() {
		fn console_handler() {
			let Some(driver) = get_console_driver() else {
				return;
			};

			driver.lock().handle_interrupt();
		}

		let irq_number = drv.lock().get_interrupt_number();
		handlers
			.entry(irq_number)
			.or_default()
			.push_back(console_handler);
	}

	#[cfg(feature = "virtio-fs")]
	if let Some(drv) = get_filesystem_driver() {
		fn virtio_fs_handler() {
			let Some(driver) = get_filesystem_driver() else {
				return;
			};

			driver.lock().handle_interrupt();
		}

		let irq_number = drv.lock().get_interrupt_number();

		handlers
			.entry(irq_number)
			.or_default()
			.push_back(virtio_fs_handler);
	}

	#[cfg(feature = "virtio-vsock")]
	if let Some(drv) = get_vsock_driver() {
		fn vsock_handler() {
			let Some(driver) = get_vsock_driver() else {
				return;
			};

			driver.lock().handle_interrupt();
		}

		let irq_number = drv.lock().get_interrupt_number();
		handlers
			.entry(irq_number)
			.or_default()
			.push_back(vsock_handler);
	}

	handlers
}
