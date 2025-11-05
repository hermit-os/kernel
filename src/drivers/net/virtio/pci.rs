use pci_types::CommandRegister;
use smoltcp::phy::ChecksumCapabilities;
use volatile::VolatileRef;

use super::{Init, Uninit};
use crate::arch::pci::PciConfigRegion;
use crate::drivers::net::virtio::{NetDevCfg, VirtioNetDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver<Uninit> {
	fn map_cfg(cap: &PciCap) -> Option<NetDevCfg> {
		let dev_cfg = pci::map_dev_cfg::<virtio::net::Config>(cap)?;

		let dev_cfg = VolatileRef::from_ref(dev_cfg);

		Some(NetDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::net::F::empty(),
		})
	}

	/// Instantiates a new (VirtioNetDriver)[VirtioNetDriver] struct, by checking the available
	/// configuration structures and moving them into the struct.
	pub(crate) fn new(
		caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioNetError> {
		let device_id = device.device_id();
		let UniCapsColl {
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg_list,
			..
		} = caps_coll;

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioNetDriver::map_cfg) else {
			error!("No dev config. Aborting!");
			return Err(error::VirtioNetError::NoDevCfg(device_id));
		};

		Ok(VirtioNetDriver {
			dev_cfg,
			com_cfg,
			isr_stat: isr_cfg,
			notif_cfg,
			inner: Uninit,
			num_vqs: 0,
			irq: device.get_irq().unwrap(),
			checksums: ChecksumCapabilities::default(),
		})
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
	) -> Result<VirtioNetDriver<Init>, VirtioError> {
		// enable bus master mode
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);

		let drv = match pci::map_caps(device) {
			Ok(caps) => match VirtioNetDriver::new(caps, device) {
				Ok(driver) => driver,
				Err(vnet_err) => {
					error!("Initializing new network driver failed. Aborting!");
					return Err(VirtioError::NetDriver(vnet_err));
				}
			},
			Err(err) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(err);
			}
		};

		let initialized_drv = match drv.init_dev() {
			Ok(initialized_drv) => {
				info!(
					"Network device with id {:x}, has been initialized by driver!",
					initialized_drv.get_dev_id()
				);
				initialized_drv
			}
			Err(vnet_err) => {
				return Err(VirtioError::NetDriver(vnet_err));
			}
		};

		if initialized_drv.is_link_up() {
			info!("Virtio-net link is up after initialization.");
		} else {
			info!("Virtio-net link is down after initialization!");
		}

		Ok(initialized_drv)
	}
}
