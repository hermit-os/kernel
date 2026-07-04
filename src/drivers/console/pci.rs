use alloc::vec::Vec;

use pci_types::CommandRegister;
use virtio::console::Config;
use volatile::VolatileRef;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::console::{ConsoleDevCfg, RxQueue, TxQueue, VirtioConsoleDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::UniCapsColl;
use crate::drivers::virtio::transport::pci::{self, PciCap};

// Backend-dependent interface for Virtio console driver
impl VirtioConsoleDriver {
	fn map_cfg(cap: &PciCap) -> Option<ConsoleDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<Config>(cap)?;
		let dev_cfg = VolatileRef::from_ref(dev_cfg);

		Some(ConsoleDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::console::F::empty(),
		})
	}

	/// Instantiates a new VirtioConsoleDriver struct, by checking the available
	/// configuration structures and moving them into the struct.
	pub fn new(
		caps_coll: UniCapsColl,
		dev_cfg_list: Vec<PciCap>,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioConsoleError> {
		let device_id = device.device_id();
		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioConsoleDriver::map_cfg) else {
			error!("No dev config. Aborting!");
			return Err(error::VirtioConsoleError::NoDevCfg(device_id));
		};

		Ok(VirtioConsoleDriver {
			dev_cfg,
			caps_coll,
			recv_vq: RxQueue::new(),
			send_vq: TxQueue::new(),
		})
	}

	/// Initializes virtio console device
	///
	/// Returns a driver instance of VirtioConsoleDriver.
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioConsoleDriver, VirtioError> {
		// enable bus master mode
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);

		let mut drv = match pci::map_caps(device) {
			Ok((caps, dev_cfg_list)) => {
				match VirtioConsoleDriver::new(caps, dev_cfg_list, device) {
					Ok(driver) => driver,
					Err(console_err) => {
						error!("Initializing new virtio console device driver failed. Aborting!");
						return Err(VirtioError::ConsoleDriver(console_err));
					}
				}
			}
			Err(err) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev(handlers, device.get_irq()) {
			Ok(()) => {
				info!(
					"Console device with id {:x}, has been initialized by driver!",
					drv.dev_cfg.dev_id
				);

				Ok(drv)
			}
			Err(console_err) => {
				drv.set_failed();
				Err(VirtioError::ConsoleDriver(console_err))
			}
		}
	}
}
