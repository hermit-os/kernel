use alloc::vec::Vec;

use volatile::VolatileRef;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci::PciCap;
use crate::drivers::virtio::transport::{UniCapsColl, pci};
use crate::drivers::vsock::{EventQueue, RxQueue, TxQueue, VirtioVsockDriver, VsockDevCfg};

impl VirtioVsockDriver {
	fn map_cfg(cap: &PciCap) -> Option<VsockDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<virtio::vsock::Config>(cap)?;

		let dev_cfg = VolatileRef::from_ref(dev_cfg);

		Some(VsockDevCfg {
			raw: dev_cfg,
			features: virtio::vsock::F::empty(),
		})
	}

	/// Instantiates a new VirtioVsockDriver struct, by checking the available
	/// configuration structures and moving them into the struct.
	pub fn new(
		caps_coll: UniCapsColl,
		dev_cfg_list: Vec<PciCap>,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioVsockError> {
		let device_id = device.device_id();

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioVsockDriver::map_cfg) else {
			error!("No dev config. Aborting!");
			return Err(error::VirtioVsockError::NoDevCfg(device_id));
		};

		Ok(VirtioVsockDriver {
			dev_cfg,
			caps_coll,
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
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioVsockDriver, VirtioError> {
		let mut drv = match pci::map_caps(device) {
			Ok((caps, dev_cfg_list)) => match VirtioVsockDriver::new(caps, dev_cfg_list, device) {
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

		match drv.init_dev(handlers, device.get_irq()) {
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
