use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::map_caps;
use crate::drivers::vsock::VirtioVsockDriver;
use crate::drivers::{InterruptHandlerMap, InterruptLine};

// Backend-dependent interface for Virtio vsock driver
impl VirtioVsockDriver {
	/// Initializes virtio vsock device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioVsockDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioVsockDriver, VirtioError> {
		let mut drv = Self::init_dev(map_caps(registers), handlers, Some(irq))
			.map_err(|(err, _)| VirtioError::VsockDriver(err))?;
		drv.caps_coll.com_cfg.print_information();
		Ok(drv)
	}
}
