//! A module containing a virtio network driver.
//!
//! The module contains ...

use pci_types::CommandRegister;
use volatile::VolatileRef;

use crate::arch::pci::PciConfigRegion;
use crate::drivers::net::virtio::{NetDevCfg, VirtioNetDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
	fn map_cfg(cap: &PciCap) -> Option<NetDevCfg> {
		let dev_cfg: &'static virtio::net::Config =
			match pci::map_dev_cfg::<virtio::net::Config>(cap) {
				Some(cfg) => cfg,
				None => return None,
			};

		let dev_cfg = VolatileRef::from_ref(dev_cfg);

		Some(NetDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::net::F::empty(),
		})
	}

	/// Instantiates a new (VirtioNetDriver)[VirtioNetDriver] struct, by checking the available
	/// configuration structures and moving them into the struct.
	///
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
	) -> Result<VirtioNetDriver, VirtioError> {
		// enable bus master mode
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);

		let UniCapsColl {
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg_list,
			..
		} = pci::map_caps(device).inspect_err(|_| error!("Mapping capabilities failed. Aborting!"))?;

		let dev_cfg = dev_cfg_list
			.iter()
			.find_map(VirtioNetDriver::map_cfg)
			.ok_or(VirtioError::NetDriver(error::VirtioNetError::NoDevCfg(
				device.device_id(),
			)))
			.inspect_err(|_| error!("No dev config. Aborting!"))?;

		VirtioNetDriver::init_dev(
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg,
			device.get_irq().unwrap(),
		)
		.map_err(VirtioError::NetDriver)
		.inspect(|drv| {
			info!(
				"Network device with id {:x}, has been initialized by driver!",
				drv.get_dev_id()
			);
			info!(
				"Virtio-net link is {} after initialization.",
				if drv.is_link_up() { "up" } else { "down" }
			);
		})
		.inspect_err(|_| error!("Initializing new network driver failed. Aborting!"))
	}
}
