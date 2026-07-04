use pci_types::CommandRegister;
use virtio::console::Config;
use volatile::VolatileRef;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::console::VirtioConsoleDriver;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci::{self, PciCap};

// Backend-dependent interface for Virtio console driver
impl VirtioConsoleDriver {
	fn map_cfg(cap: &PciCap) -> Option<VolatileRef<'static, Config, volatile::access::ReadOnly>> {
		let dev_cfg = pci::map_dev_cfg(cap)?;
		Some(VolatileRef::from_ref(dev_cfg))
	}

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
			.find_map(VirtioConsoleDriver::map_cfg)
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
