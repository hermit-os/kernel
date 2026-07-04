use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::map_caps;
use crate::drivers::{InterruptHandlerMap, InterruptLine};

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver {
	/// Initializes virtio network device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioNetDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioNetDriver, VirtioError> {
		let mut drv = VirtioNetDriver::init_dev(map_caps(registers), handlers, Some(irq))
			.map_err(VirtioError::NetDriver)?;
		drv.print_information();
		Ok(drv)
	}

	pub fn print_information(&mut self) {
		self.caps_coll.com_cfg.print_information();
		if self.dev_status() == virtio::net::S::LINK_UP {
			info!("The link of the network device is up!");
		}
	}
}
