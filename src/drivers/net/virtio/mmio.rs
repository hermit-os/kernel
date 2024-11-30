//! A module containing a virtio network driver.
//!
//! The module contains ...

use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::drivers::net::virtio::{NetDevCfg, VirtioNetDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::InterruptLine;

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
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
		mut registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<VirtioNetDriver, VirtioError> {
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

		let mut drv =
			VirtioNetDriver::init_dev(ComCfg::new(registers, 1), notif_cfg, isr_stat, dev_cfg, irq)
				.map_err(VirtioError::NetDriver)
				.inspect_err(|_| error!("Unable to create Driver. Aborting!"))?;
		drv.print_information();
		Ok(drv)
	}
}
