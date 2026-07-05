use pci_types::CommandRegister;

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
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
			.find_map(|cap| cap.map_cap_cfg().ok())
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
