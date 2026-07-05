use pci_types::CommandRegister;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::console::VirtioConsoleDriver;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;

// Backend-dependent interface for Virtio console driver
impl VirtioConsoleDriver {
	/// Initializes virtio console device by checking the available
	/// configuration structures and calling the initializer on them.
	///
	/// Returns a driver instance of VirtioConsoleDriver.
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioConsoleDriver, VirtioError> {
		// enable bus master mode
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);

		let (caps, dev_cfg_list) = pci::map_caps(device)
			.inspect_err(|_| error!("Mapping capabilities failed. Aborting!"))?;

		let dev_cfg = dev_cfg_list
			.iter()
			.find_map(|cap| cap.map_cap_cfg().ok())
			.ok_or_else(|| {
				error!("No dev config. Aborting!");
				error!("Initializing new virtio console device driver failed. Aborting!");
				VirtioError::ConsoleDriver(error::VirtioConsoleError::NoDevCfg(device.device_id()))
			})?;

		match Self::init_dev((caps, dev_cfg), handlers, device.get_irq()) {
			Ok(drv) => {
				info!("Console device has been initialized by driver!",);

				Ok(drv)
			}
			Err((console_err, mut caps_coll)) => {
				caps_coll.com_cfg.set_failed();
				Err(VirtioError::ConsoleDriver(console_err))
			}
		}
	}
}
