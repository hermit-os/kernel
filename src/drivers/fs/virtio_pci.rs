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
		let dev_cfg = match pci::map_dev_cfg::<virtio::fs::Config>(cap) {
			Some(cfg) => cfg,
			None => return None,
		};

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
		mut caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioFsError> {
		let device_id = device.device_id();

		let com_cfg = match caps_coll.get_com_cfg() {
			Some(com_cfg) => com_cfg,
			None => {
				error!("No common config. Aborting!");
				return Err(error::VirtioFsError::NoComCfg(device_id));
			}
		};

		let isr_stat = match caps_coll.get_isr_cfg() {
			Some(isr_stat) => isr_stat,
			None => {
				error!("No ISR status config. Aborting!");
				return Err(error::VirtioFsError::NoIsrCfg(device_id));
			}
		};

		let notif_cfg = match caps_coll.get_notif_cfg() {
			Some(notif_cfg) => notif_cfg,
			None => {
				error!("No notif config. Aborting!");
				return Err(error::VirtioFsError::NoNotifCfg(device_id));
			}
		};

		let dev_cfg = loop {
			match caps_coll.get_dev_cfg() {
				Some(cfg) => {
					if let Some(dev_cfg) = VirtioFsDriver::map_cfg(&cfg) {
						break dev_cfg;
					}
				}
				None => {
					error!("No dev config. Aborting!");
					return Err(error::VirtioFsError::NoDevCfg(device_id));
				}
			}
		};

		Ok(VirtioFsDriver {
			dev_cfg,
			com_cfg,
			isr_stat,
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
					error!("Initializing new network driver failed. Aborting!");
					return Err(VirtioError::FsDriver(fs_err));
				}
			},
			Err(pci_error) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(VirtioError::FromPci(pci_error));
			}
		};

		match drv.init_dev() {
			Ok(_) => info!(
				"Filesystem device with id {:x}, has been initialized by driver!",
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
