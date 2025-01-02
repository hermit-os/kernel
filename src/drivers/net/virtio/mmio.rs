//! A module containing a virtio network driver.
//!
//! The module contains ...

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::str::FromStr;

use smoltcp::phy::ChecksumCapabilities;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::drivers::InterruptLine;
use crate::drivers::net::virtio::{CtrlQueue, NetDevCfg, RxQueues, TxQueues, VirtioNetDriver};
use crate::drivers::virtio::error::{VirtioError, VirtioNetError};
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::Virtq;

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
	pub fn new(
		dev_id: u16,
		mut registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<Self, VirtioNetError> {
		let dev_cfg_raw: &'static virtio::net::Config = unsafe {
			&*registers
				.borrow_mut()
				.as_mut_ptr()
				.config()
				.as_raw_ptr()
				.cast::<virtio::net::Config>()
				.as_ptr()
		};
		let dev_cfg_raw = VolatileRef::from_ref(dev_cfg_raw);
		let dev_cfg = NetDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::net::F::empty(),
		};
		let isr_stat = IsrStatus::new(registers.borrow_mut());
		let notif_cfg = NotifCfg::new(registers.borrow_mut());

		let mtu = if let Some(my_mtu) = hermit_var!("HERMIT_MTU") {
			u16::from_str(&my_mtu).unwrap()
		} else {
			// fallback to the default MTU
			1514
		};

		let send_vqs = TxQueues::new(Vec::<Box<dyn Virtq>>::new(), &dev_cfg);
		let recv_vqs = RxQueues::new(Vec::<Box<dyn Virtq>>::new(), &dev_cfg);
		Ok(VirtioNetDriver {
			dev_cfg,
			com_cfg: ComCfg::new(registers, 1),
			isr_stat,
			notif_cfg,
			ctrl_vq: CtrlQueue::new(None),
			recv_vqs,
			send_vqs,
			num_vqs: 0,
			mtu,
			irq,
			checksums: ChecksumCapabilities::default(),
		})
	}

	pub fn print_information(&mut self) {
		self.com_cfg.print_information();
		if self.dev_status() == virtio::net::S::LINK_UP {
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
		irq: InterruptLine,
	) -> Result<VirtioNetDriver, VirtioError> {
		if let Ok(mut drv) = VirtioNetDriver::new(dev_id, registers, irq) {
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
