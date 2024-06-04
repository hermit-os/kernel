//! A module containing a virtio network driver.
//!
//! The module contains ...

use alloc::vec::Vec;
use core::str::FromStr;

use pci_types::CommandRegister;
use smoltcp::phy::ChecksumCapabilities;

use crate::arch::pci::PciConfigRegion;
use crate::drivers::net::virtio_net::{CtrlQueue, NetDevCfg, RxQueues, TxQueues, VirtioNetDriver};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::{self, VirtioError};
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{PciCap, UniCapsColl};

/// Virtio's network device configuration structure.
/// See specification v1.1. - 5.1.4
///
#[repr(C)]
pub(crate) struct NetDevCfgRaw {
	// Specifies Mac address, only Valid if VIRTIO_NET_F_MAC is set
	mac: [u8; 6],
	// Indicates status of device. Only valid if VIRTIO_NET_F_STATUS is set
	status: u16,
	// Indicates number of allowed vq-pairs. Only valid if VIRTIO_NET_F_MQ is set.
	max_virtqueue_pairs: u16,
	// Indicates the maximum MTU driver should use. Only valid if VIRTIONET_F_MTU is set.
	mtu: u16,
}

impl NetDevCfgRaw {
	pub fn get_mtu(&self) -> u16 {
		self.mtu
	}

	pub fn get_mac(&self) -> [u8; 6] {
		self.mac
	}

	pub fn get_status(&self) -> u16 {
		self.status
	}

	pub fn get_max_virtqueue_pairs(&self) -> u16 {
		self.max_virtqueue_pairs
	}
}

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
	fn map_cfg(cap: &PciCap) -> Option<NetDevCfg> {
		/*
		if cap.bar_len() <  u64::from(cap.len() + cap.offset()) {
			error!("Network config of device {:x}, does not fit into memory specified by bar!",
				cap.dev_id(),
			);
			return None
		}

		// Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
		if cap.len() < MemLen::from(mem::size_of::<NetDevCfg>()) {
			error!("Network config from device {:x}, does not represent actual structure specified by the standard!", cap.dev_id());
			return None
		}

		let virt_addr_raw = cap.bar_addr() + cap.offset();

		// Create mutable reference to the PCI structure in PCI memory
		let dev_cfg: &mut NetDevCfgRaw = unsafe {
			&mut *(usize::from(virt_addr_raw) as *mut NetDevCfgRaw)
		};
		*/
		let dev_cfg: &'static NetDevCfgRaw = match pci::map_dev_cfg::<NetDevCfgRaw>(cap) {
			Some(cfg) => cfg,
			None => return None,
		};

		Some(NetDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio_spec::net::F::empty(),
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

		Ok(VirtioNetDriver {
			dev_cfg,
			com_cfg,
			isr_stat,
			notif_cfg,

			ctrl_vq: CtrlQueue::new(None),
			recv_vqs: RxQueues::new(Vec::new(), false),
			send_vqs: TxQueues::new(Vec::new(), Vec::new(), false),
			num_vqs: 0,
			irq: device.get_irq().unwrap(),
			mtu,
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
