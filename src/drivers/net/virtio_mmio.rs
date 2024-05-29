//! A module containing a virtio network driver.
//!
//! The module contains ...

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::ptr::read_volatile;
use core::str::FromStr;
use core::sync::atomic::{fence, Ordering};

use smoltcp::phy::ChecksumCapabilities;
use virtio_spec::mmio::{DeviceRegisterVolatileFieldAccess, DeviceRegisters};
use volatile::VolatileRef;

use crate::drivers::net::virtio_net::constants::Status;
use crate::drivers::net::virtio_net::{CtrlQueue, NetDevCfg, RxQueues, TxQueues, VirtioNetDriver};
use crate::drivers::virtio::error::{VirtioError, VirtioNetError};
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::Virtq;

/// Virtio's network device configuration structure.
/// See specification v1.1. - 5.1.4
///
#[repr(C)]
pub struct NetDevCfgRaw {
	config_generation: u32,
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
		// see Virtio specification v1.1 -  2.4.1
		unsafe {
			loop {
				let before = read_volatile(&self.config_generation);
				fence(Ordering::SeqCst);
				let mtu = read_volatile(&self.mtu);
				fence(Ordering::SeqCst);
				let after = read_volatile(&self.config_generation);

				if before == after {
					return mtu;
				}
			}
		}
	}

	pub fn get_mac(&self) -> [u8; 6] {
		let mut mac: [u8; 6] = [0u8; 6];

		// see Virtio specification v1.1 -  2.4.1
		unsafe {
			loop {
				let before = read_volatile(&self.config_generation);
				fence(Ordering::SeqCst);
				let mut src = self.mac.iter();
				mac.fill_with(|| read_volatile(src.next().unwrap()));
				fence(Ordering::SeqCst);
				let after = read_volatile(&self.config_generation);

				if before == after {
					return mac;
				}
			}
		}
	}

	pub fn get_status(&self) -> u16 {
		// see Virtio specification v1.1 -  2.4.1
		unsafe {
			loop {
				let before = read_volatile(&self.config_generation);
				fence(Ordering::SeqCst);
				let status = read_volatile(&self.status);
				fence(Ordering::SeqCst);
				let after = read_volatile(&self.config_generation);

				if before == after {
					return status;
				}
			}
		}
	}

	pub fn get_max_virtqueue_pairs(&self) -> u16 {
		// see Virtio specification v1.1 -  2.4.1
		unsafe {
			loop {
				let before = read_volatile(&self.config_generation);
				fence(Ordering::SeqCst);
				let max_pairs = read_volatile(&self.max_virtqueue_pairs);
				fence(Ordering::SeqCst);
				let after = read_volatile(&self.config_generation);

				if before == after {
					return max_pairs;
				}
			}
		}
	}
}

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
	pub fn new(
		dev_id: u16,
		mut registers: VolatileRef<'static, DeviceRegisters>,
		irq: u8,
	) -> Result<Self, VirtioNetError> {
		let dev_cfg_raw: &'static NetDevCfgRaw = unsafe {
			&*registers
				.borrow_mut()
				.as_mut_ptr()
				.config_generation()
				.as_raw_ptr()
				.cast::<NetDevCfgRaw>()
				.as_ptr()
		};
		let dev_cfg = NetDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio_spec::net::F::empty(),
		};
		let isr_stat = IsrStatus::new(registers.borrow_mut());
		let notif_cfg = NotifCfg::new(registers.borrow_mut());

		let mtu = if let Some(my_mtu) = hermit_var!("HERMIT_MTU") {
			u16::from_str(&my_mtu).unwrap()
		} else {
			// fallback to the default MTU
			1514
		};

		Ok(VirtioNetDriver {
			dev_cfg,
			com_cfg: ComCfg::new(registers, 1),
			isr_stat,
			notif_cfg,
			ctrl_vq: CtrlQueue::new(None),
			recv_vqs: RxQueues::new(Vec::<Rc<dyn Virtq>>::new(), false),
			send_vqs: TxQueues::new(Vec::<Rc<dyn Virtq>>::new(), Vec::new(), false),
			num_vqs: 0,
			irq,
			mtu,
			checksums: ChecksumCapabilities::default(),
		})
	}

	pub fn print_information(&mut self) {
		self.com_cfg.print_information();
		if self.dev_status() == u16::from(Status::VIRTIO_NET_S_LINK_UP) {
			info!("The link of the network device is up!");
		}
	}

	/// Initializes virtio network device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioNetDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
		irq_no: u8,
	) -> Result<VirtioNetDriver, VirtioError> {
		if let Ok(mut drv) = VirtioNetDriver::new(dev_id, registers, irq_no) {
			match drv.init_dev() {
				Err(error_code) => Err(VirtioError::NetDriver(error_code)),
				_ => {
					drv.print_information();
					Ok(drv)
				}
			}
		} else {
			error!("Unable to create Driver. Aborting!");
			Err(VirtioError::Unknown)
		}
	}
}
