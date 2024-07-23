use alloc::vec::Vec;

use crate::arch::pci::PciConfigRegion;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};
use crate::drivers::vsock::{VirtioVsockDriver, VsockDevCfg};

/// Virtio's socket device configuration structure.
/// See specification v1.1. - 5.11.4
///
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub(crate) struct VsockDevCfgRaw {
	/// The guest_cid field contains the guest’s context ID, which uniquely identifies the device
	/// for its lifetime. The upper 32 bits of the CID are reserved and zeroed.
	guest_cid: u64,
}

impl VirtioVsockDriver {
	fn map_cfg(cap: &PciCap) -> Option<VsockDevCfg> {
		let dev_cfg: &'static VsockDevCfgRaw = match pci::map_dev_cfg::<VsockDevCfgRaw>(cap) {
			Some(cfg) => cfg,
			None => return None,
		};

		Some(VsockDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::net::F::empty(),
		})
	}

	/// Instantiates a new VirtioVsockDriver struct, by checking the available
	/// configuration structures and moving them into the struct.
	pub fn new(
		mut caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioVsockError> {
		let device_id = device.device_id();

		let com_cfg = match caps_coll.get_com_cfg() {
			Some(com_cfg) => com_cfg,
			None => {
				error!("No common config. Aborting!");
				return Err(error::VirtioVsockError::NoComCfg(device_id));
			}
		};

		let isr_stat = match caps_coll.get_isr_cfg() {
			Some(isr_stat) => isr_stat,
			None => {
				error!("No ISR status config. Aborting!");
				return Err(error::VirtioVsockError::NoIsrCfg(device_id));
			}
		};

		let notif_cfg = match caps_coll.get_notif_cfg() {
			Some(notif_cfg) => notif_cfg,
			None => {
				error!("No notif config. Aborting!");
				return Err(error::VirtioVsockError::NoNotifCfg(device_id));
			}
		};

		let dev_cfg = loop {
			match caps_coll.get_dev_cfg() {
				Some(cfg) => {
					if let Some(dev_cfg) = VirtioVsockDriver::map_cfg(&cfg) {
						break dev_cfg;
					}
				}
				None => {
					error!("No dev config. Aborting!");
					return Err(error::VirtioVsockError::NoDevCfg(device_id));
				}
			}
		};

		Ok(VirtioVsockDriver {
			dev_cfg,
			com_cfg,
			isr_stat,
			notif_cfg,
			irq: device.get_irq().unwrap(),
			vqueues: Vec::new(),
		})
	}

	/// Initializes virtio socket device
	///
	/// Returns a driver instance of VirtioVsockDriver.
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
	) -> Result<VirtioVsockDriver, VirtioError> {
		let mut drv = match pci::map_caps(device) {
			Ok(caps) => match VirtioVsockDriver::new(caps, device) {
				Ok(driver) => driver,
				Err(vsock_err) => {
					error!("Initializing new virtio socket device driver failed. Aborting!");
					return Err(VirtioError::VsockDriver(vsock_err));
				}
			},
			Err(pci_error) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(VirtioError::FromPci(pci_error));
			}
		};

		match drv.init_dev() {
			Ok(_) => {
				info!(
					"Socket device with cid {:x}, has been initialized by driver!",
					drv.dev_cfg.raw.guest_cid
				);
				Ok(drv)
			}
			Err(fs_err) => {
				drv.set_failed();
				Err(VirtioError::VsockDriver(fs_err))
			}
		}
	}
}
