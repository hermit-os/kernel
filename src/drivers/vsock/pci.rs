use volatile::VolatileRef;

use crate::arch::pci::PciConfigRegion;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};
use crate::drivers::vsock::{EventQueue, RxQueue, TxQueue, VirtioVsockDriver, VsockDevCfg};

impl VirtioVsockDriver {
	fn map_cfg(cap: &PciCap) -> Option<VsockDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<virtio::vsock::Config>(cap)?;

		let dev_cfg = VolatileRef::from_ref(dev_cfg);

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
			error!("No dev config. Aborting!");
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
					error!("Initializing new virtio socket device driver failed. Aborting!");
					return Err(VirtioError::VsockDriver(vsock_err));
				}
			},
			Err(err) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		match drv.init_dev() {
			Ok(()) => {
				let cid = drv.get_cid();
				info!("Socket device with cid {cid:x}, has been initialized by driver!");

				Ok(drv)
			}
			Err(fs_err) => {
				drv.set_failed();
				Err(VirtioError::VsockDriver(fs_err))
			}
		}
	}
}
