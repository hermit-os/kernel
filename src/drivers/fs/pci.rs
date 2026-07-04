use volatile::VolatileRef;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::fs::VirtioFsDriver;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::PciCap;

impl VirtioFsDriver {
	fn map_cfg(
		cap: &PciCap,
	) -> Option<VolatileRef<'static, virtio::fs::Config, volatile::access::ReadOnly>> {
		let dev_cfg = pci::map_dev_cfg(cap)?;
		Some(VolatileRef::from_ref(dev_cfg))
	}

	/// Initializes virtio filesystem device by checking the available
	/// configuration structures and calling the initializer on them.
	pub fn init(
		device: &PciDevice<PciConfigRegion>,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioFsDriver, VirtioError> {
		let (caps, dev_cfg_list) = pci::map_caps(device)
			.inspect_err(|_| error!("Mapping capabilities failed. Aborting!"))?;

		let dev_cfg = dev_cfg_list
			.iter()
			.find_map(VirtioFsDriver::map_cfg)
			.ok_or_else(|| {
				error!("No dev config. Aborting!");
				error!("Initializing new network driver failed. Aborting!");
				VirtioError::FsDriver(error::VirtioFsInitError::NoDevCfg(device.device_id()))
			})?;

		match Self::init_dev((caps, dev_cfg), handlers, device.get_irq()) {
			Ok(drv) => {
				info!("Filesystem device has been initialized by driver!",);
				Ok(drv)
			}
			Err((fs_err, mut caps_coll)) => {
				caps_coll.com_cfg.set_failed();
				Err(VirtioError::FsDriver(fs_err))
			}
		}
	}
}
