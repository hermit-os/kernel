use crate::arch::pci::PciConfigRegion;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};
use crate::drivers::vsock::{EventQueue, RxQueue, TxQueue, VirtioVsockDriver, VsockDevCfg};

/// Virtio's socket device configuration structure.
/// See specification v1.1. - 5.11.4
///
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub(crate) struct VsockDevCfgRaw {
	/// The guest_cid field contains the guest’s context ID, which uniquely identifies the device
	/// for its lifetime. The upper 32 bits of the CID are reserved and zeroed.
	pub guest_cid: u64,
}

impl VirtioVsockDriver {
	fn map_cfg(cap: &PciCap) -> Option<VsockDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<VsockDevCfgRaw>(cap)?;

		Some(VsockDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::vsock::F::empty(),
		})
	}

	/// Instantiates a new VirtioVsockDriver struct, by checking the available
	/// configuration structures and moving them into the struct.
	pub fn new(
		caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioVsockError> {
		let device_id = device.device_id();

		let UniCapsColl {
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg_list,
			..
		} = caps_coll;

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioVsockDriver::map_cfg) else {
			error!("No dev config present. Aborting!");
			return Err(error::VirtioVsockError::NoDevCfg(device_id));
		};

		Ok(VirtioVsockDriver {
			dev_cfg,
			com_cfg,
			isr_stat: isr_cfg,
			notif_cfg,
			irq: device.get_irq().unwrap(),
			event_vq: EventQueue::new(),
			recv_vq: RxQueue::new(),
			send_vq: TxQueue::new(),
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
					error!("vsock: Driver initialization failed. Aborting!");
					return Err(VirtioError::VsockDriver(vsock_err));
				}
			},
			Err(err) => {
				error!("vsock: Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev() {
			Ok(()) => {
				info!(
					"vsock: Driver initialized socket device with cid {:x}",
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
