use volatile::VolatileRef;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::PciCap;
use crate::drivers::vsock::VirtioVsockDriver;

impl VirtioVsockDriver {
	fn map_cfg(
		cap: &PciCap,
	) -> Option<VolatileRef<'static, virtio::vsock::Config, volatile::access::ReadOnly>> {
		let dev_cfg = pci::map_dev_cfg(cap)?;
		Some(VolatileRef::from_ref(dev_cfg))
	}

	/// Initializes virtio socket device by checking the available
	/// configuration structures and calling the initializer on them.
	///
	/// Returns a driver instance of VirtioVsockDriver.
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioVsockDriver, VirtioError> {
		let (caps, dev_cfg_list) = pci::map_caps(device)
			.inspect_err(|_| error!("Mapping capabilities failed. Aborting!"))?;

		let dev_cfg = dev_cfg_list
			.iter()
			.find_map(VirtioVsockDriver::map_cfg)
			.ok_or_else(|| {
				error!("No dev config. Aborting!");
				error!("Initializing new virtio socket device driver failed. Aborting!");
				VirtioError::VsockDriver(error::VirtioVsockError::NoDevCfg(device.device_id()))
			})?;

		match Self::init_dev((caps, dev_cfg), handlers, device.get_irq()) {
			Ok(drv) => {
				let cid = drv.get_cid();
				info!("Socket device with cid {cid:x}, has been initialized by driver!");

				Ok(drv)
			}
			Err((fs_err, mut caps_coll)) => {
				caps_coll.com_cfg.set_failed();
				Err(VirtioError::VsockDriver(fs_err))
			}
		}
	}
}
