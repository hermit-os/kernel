use pci_types::CommandRegister;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::entropy::{EntropyDevCfg, RxQueue, VirtioEntropyDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci::{self, PciCap, UniCapsColl};

impl VirtioEntropyDriver {
	fn map_cfg(cap: &PciCap) -> Option<EntropyDevCfg> {
		Some(EntropyDevCfg {
			dev_id: cap.dev_id(),
			features: virtio::entropy::F::empty(),
		})
	}

	pub fn new(
		caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioEntropyError> {
		let device_id = device.device_id();

		let UniCapsColl {
			com_cfg,
			notif_cfg,
			dev_cfg_list,
			..
		} = caps_coll;

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioEntropyDriver::map_cfg) else {
			error!("No dev config. Aborting!");
			return Err(error::VirtioEntropyError::NoDevCfg(device_id));
		};

		Ok(VirtioEntropyDriver {
			dev_cfg,
			com_cfg,
			notif_cfg,
			recv_vq: RxQueue::new(),
		})
	}

	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
	) -> Result<VirtioEntropyDriver, VirtioError> {
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);

		let mut drv = match pci::map_caps(device) {
			Ok(caps) => match VirtioEntropyDriver::new(caps, device) {
				Ok(driver) => driver,
				Err(entropy_err) => {
					error!("Initializing new virtio entropy device driver failed. Aborting!");
					return Err(VirtioError::EntropyDriver(entropy_err));
				}
			},
			Err(err) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev() {
			Ok(()) => {
				info!(
					"Entropy device with id {:x}, has been initialized by driver!",
					drv.dev_cfg.dev_id
				);

				Ok(drv)
			}
			Err(entropy_err) => {
				drv.set_failed();
				Err(VirtioError::EntropyDriver(entropy_err))
			}
		}
	}
}
