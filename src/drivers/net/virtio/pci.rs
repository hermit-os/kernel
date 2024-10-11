//! A module containing a virtio network driver.
//!
//! The module contains ...

use alloc::vec::Vec;
use core::str::FromStr;

use pci_types::CommandRegister;
use smoltcp::phy::ChecksumCapabilities;
use volatile::VolatileRef;

use crate::arch::pci::PciConfigRegion;
use crate::drivers::net::virtio::{CtrlQueue, NetDevCfg, RxQueues, TxQueues, VirtioNetDriver};
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
	pub(crate) fn new(
		mut caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, error::VirtioNetError> {
		let device_id = device.device_id();
		let com_cfg = match caps_coll.get_com_cfg() {
			Some(com_cfg) => com_cfg,
			None => {
				error!("No common config. Aborting!");
				return Err(error::VirtioNetError::NoComCfg(device_id));
			}
		};

		let isr_stat = match caps_coll.get_isr_cfg() {
			Some(isr_stat) => isr_stat,
			None => {
				error!("No ISR status config. Aborting!");
				return Err(error::VirtioNetError::NoIsrCfg(device_id));
			}
		};

		let notif_cfg = match caps_coll.get_notif_cfg() {
			Some(notif_cfg) => notif_cfg,
			None => {
				error!("No notif config. Aborting!");
				return Err(error::VirtioNetError::NoNotifCfg(device_id));
			}
		};

		let dev_cfg = loop {
			match caps_coll.get_dev_cfg() {
				Some(cfg) => {
					if let Some(dev_cfg) = VirtioNetDriver::map_cfg(&cfg) {
						break dev_cfg;
					}
				}
				None => {
					error!("No dev config. Aborting!");
					return Err(error::VirtioNetError::NoDevCfg(device_id));
				}
			}
		};

		let mtu = if let Some(my_mtu) = hermit_var!("HERMIT_MTU") {
			u16::from_str(&my_mtu).unwrap()
		} else {
			// fallback to the default MTU
			1514
		};

		let send_vqs = TxQueues::new(Vec::new(), &dev_cfg);
		let recv_vqs = RxQueues::new(Vec::new(), &dev_cfg);
		Ok(VirtioNetDriver {
			dev_cfg,
			com_cfg,
			isr_stat,
			notif_cfg,
			ctrl_vq: CtrlQueue::new(None),
			recv_vqs,
			send_vqs,
			num_vqs: 0,
			mtu,
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
	) -> Result<VirtioNetDriver, VirtioError> {
		// enable bus master mode
		device.set_command(CommandRegister::BUS_MASTER_ENABLE);

		let mut drv = match pci::map_caps(device) {
			Ok(caps) => match VirtioNetDriver::new(caps, device) {
				Ok(driver) => driver,
				Err(vnet_err) => {
					error!("Initializing new network driver failed. Aborting!");
					return Err(VirtioError::NetDriver(vnet_err));
				}
			},
			Err(pci_error) => {
				error!("Mapping capabilities failed. Aborting!");
				return Err(VirtioError::FromPci(pci_error));
			}
		};

		match drv.init_dev() {
			Ok(_) => info!(
				"Network device with id {:x}, has been initialized by driver!",
				drv.get_dev_id()
			),
			Err(vnet_err) => {
				drv.set_failed();
				return Err(VirtioError::NetDriver(vnet_err));
			}
		}

		if drv.is_link_up() {
			info!("Virtio-net link is up after initialization.")
		} else {
			info!("Virtio-net link is down after initialization!")
		}

		Ok(drv)
	}
}
