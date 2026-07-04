use alloc::vec::Vec;

use volatile::VolatileRef;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::fs::{FsDevCfg, VirtioFsDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci::PciCap;
use crate::drivers::virtio::transport::{UniCapsColl, pci};

impl VirtioFsDriver {
	fn map_cfg(cap: &PciCap) -> Option<FsDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<virtio::fs::Config>(cap)?;

		let dev_cfg = VolatileRef::from_ref(dev_cfg);

		Some(FsDevCfg {
			raw: dev_cfg,
			features: virtio::fs::F::empty(),
		})
	}

	/// Instantiates a new (VirtioFsDriver)[VirtioFsDriver] struct, by checking the available
	/// configuration structures and moving them into the struct.
	pub fn new(
		caps_coll: UniCapsColl,
		dev_cfg_list: Vec<PciCap>,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioFsInitError> {
		let device_id = device.device_id();

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioFsDriver::map_cfg) else {
			error!("No dev config. Aborting!");
			return Err(error::VirtioFsInitError::NoDevCfg(device_id));
		};

		Ok(VirtioFsDriver {
			dev_cfg,
			caps_coll,
			vqueues: Vec::new(),
		})
	}

	/// Initializes virtio filesystem device
	pub fn init(
		device: &PciDevice<PciConfigRegion>,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioFsDriver, VirtioError> {
		let mut drv = match pci::map_caps(device) {
			Ok((caps, dev_cfg_list)) => match VirtioFsDriver::new(caps, dev_cfg_list, device) {
				Ok(driver) => driver,
				Err(fs_err) => {
					error!("Initializing new network driver failed. Aborting!");
					return Err(VirtioError::FsDriver(fs_err));
				}
			},
			Err(err) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev(handlers, device.get_irq()) {
			Ok(()) => info!("Filesystem device has been initialized by driver!",),
			Err(fs_err) => {
				drv.set_failed();
				return Err(VirtioError::FsDriver(fs_err));
			}
		}

		Ok(drv)
	}
}
