use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::fs::VirtioFsDriver;
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::map_caps;
use crate::drivers::{InterruptHandlerMap, InterruptLine};

// Backend-dependent interface for Virtio fs driver
impl VirtioFsDriver {
	/// Initializes virtio fs device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioFsDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioFsDriver, VirtioError> {
		let mut drv = Self::init_dev(map_caps(registers), handlers, Some(irq))
			.map_err(|(err, _)| VirtioError::FsDriver(err))?;
		drv.print_information();
		Ok(drv)
	}

	fn print_information(&mut self) {
		self.caps_coll.com_cfg.print_information();
	}
}
