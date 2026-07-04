use pci_types::CommandRegister;
use volatile::VolatileRef;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::PciCap;

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
	fn map_cfg(
		cap: &PciCap,
	) -> Option<VolatileRef<'static, virtio::net::Config, volatile::access::ReadOnly>> {
		let dev_cfg = pci::map_dev_cfg(cap)?;
		Some(VolatileRef::from_ref(dev_cfg))
	}

	/// Initializes virtio network device by mapping configuration layout to
	/// respective structs (configuration structs are:
	/// [ComCfg](structs.comcfg.html), [NotifCfg](structs.notifcfg.html)
	/// [IsrStatus](structs.isrstatus.html), [PciCfg](structs.pcicfg.html)
	/// [ShMemCfg](structs.ShMemCfg)).
	///
	/// Returns a driver instance of
	/// [VirtioNetDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub(crate) fn init(
		device: &PciDevice<PciConfigRegion>,
		handlers: &mut InterruptHandlerMap,
	) -> Result<Self, VirtioError> {
		// enable bus master mode
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);
		let (caps, dev_cfg_list) = pci::map_caps(device)
			.inspect_err(|_| error!("Mapping capabilities failed. Aborting!"))?;

		let dev_cfg = dev_cfg_list
			.iter()
			.find_map(VirtioNetDriver::map_cfg)
			.ok_or_else(|| {
				error!("No dev config. Aborting!");
				error!("Initializing new network driver failed. Aborting!");
				VirtioError::NetDriver(error::VirtioNetError::NoDevCfg(device.device_id()))
			})?;

		let initialized_drv = Self::init_dev((caps, dev_cfg), handlers, device.get_irq())
			.map_err(VirtioError::NetDriver)?;
		info!("Network device has been initialized by driver!",);

		if initialized_drv.is_link_up() {
			info!("Virtio-net link is up after initialization.");
		} else {
			info!("Virtio-net link is down after initialization!");
		}

		Ok(initialized_drv)
	}
}
