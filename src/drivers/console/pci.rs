use pci_types::CommandRegister;
use virtio::console::Config;
use volatile::VolatileRef;

use crate::drivers::console::{ConsoleDevCfg, RxQueue, TxQueue, VirtioConsoleDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci::{self, PciCap, UniCapsColl};
use crate::pci::PciConfigRegion;

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
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioConsoleError> {
		let device_id = device.device_id();

		let UniCapsColl {
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg_list,
			..
		} = caps_coll;

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioConsoleDriver::map_cfg) else {
			error!("virtio-console: No dev config present. Aborting!");
			return Err(error::VirtioConsoleError::NoDevCfg(device_id));
		};

		Ok(VirtioConsoleDriver {
			dev_cfg,
			com_cfg,
			isr_stat: isr_cfg,
			notif_cfg,
			irq: device.get_irq().unwrap(),
			recv_vq: RxQueue::new(),
			send_vq: TxQueue::new(),
		})
	}

	/// Initializes virtio console device
	///
	/// Returns a driver instance of VirtioConsoleDriver.
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
	) -> Result<VirtioConsoleDriver, VirtioError> {
		// enable bus master mode
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);

		let mut drv = match pci::map_caps(device) {
			Ok(caps) => match VirtioConsoleDriver::new(caps, device) {
				Ok(driver) => driver,
				Err(console_err) => {
					error!("virtio-console: Driver initialization failed. Aborting!");
					return Err(VirtioError::ConsoleDriver(console_err));
				}
			},
			Err(err) => {
				error!("virtio-console: Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev() {
			Ok(()) => {
				info!(
					"Console device with ID {:x}, has been initialized by driver!",
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
