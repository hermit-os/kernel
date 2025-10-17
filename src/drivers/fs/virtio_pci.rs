use alloc::vec::Vec;

use volatile::VolatileRef;

use crate::arch::pci::PciConfigRegion;
use crate::drivers::fs::virtio_fs::{FsDevCfg, VirtioFsDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};

impl VirtioFsDriver {
	fn map_cfg(cap: &PciCap) -> Option<FsDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<virtio::fs::Config>(cap)?;

		let dev_cfg = VolatileRef::from_ref(dev_cfg);

		Some(FsDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::fs::F::empty(),
		})
	}

	/// Instantiates a new (VirtioFsDriver)[VirtioFsDriver] struct, by checking the available
	/// configuration structures and moving them into the struct.
	pub fn new(
		caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioFsError> {
		let device_id = device.device_id();

		let UniCapsColl {
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg_list,
			..
		} = caps_coll;

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioFsDriver::map_cfg) else {
			error!("virtiofs: No dev config present. Aborting!");
			return Err(error::VirtioFsError::NoDevCfg(device_id));
		};

		Ok(VirtioFsDriver {
			dev_cfg,
			com_cfg,
			isr_stat: isr_cfg,
			notif_cfg,
			vqueues: Vec::new(),
			irq: device.get_irq().unwrap(),
		})
	}

	/// Initializes virtio filesystem device
	pub fn init(device: &PciDevice<PciConfigRegion>) -> Result<VirtioFsDriver, VirtioError> {
		let mut drv = match pci::map_caps(device) {
			Ok(caps) => match VirtioFsDriver::new(caps, device) {
				Ok(driver) => driver,
				Err(fs_err) => {
					error!("virtiofs: Driver initialization failed. Aborting!");
					return Err(VirtioError::FsDriver(fs_err));
				}
			},
			Err(err) => {
				error!("virtiofs: Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev() {
			Ok(()) => info!(
				"virtiofs: Driver initialized filesystem device with ID {:x}!",
				drv.get_dev_id()
			),
			Err(fs_err) => {
				drv.set_failed();
				return Err(VirtioError::FsDriver(fs_err));
			}
		}

		Ok(drv)
	}
}
