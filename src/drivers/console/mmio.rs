use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::console::{ConsoleDevCfg, RxQueue, TxQueue, VirtioConsoleDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::map_caps;
use crate::drivers::{InterruptHandlerMap, InterruptLine};

// Backend-dependent interface for Virtio console driver
impl VirtioConsoleDriver {
	pub fn new(
		registers: VolatileRef<'static, DeviceRegisters>,
	) -> Result<VirtioConsoleDriver, VirtioError> {
		let (caps_coll, dev_cfg_raw) = map_caps(registers);
		let dev_cfg = ConsoleDevCfg {
			raw: dev_cfg_raw,
			features: virtio::console::F::empty(),
		};

		Ok(VirtioConsoleDriver {
			dev_cfg,
			caps_coll,
			recv_vq: RxQueue::new(),
			send_vq: TxQueue::new(),
		})
	}

	/// Initializes virtio console device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioConsoleDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioConsoleDriver, VirtioError> {
		let mut drv = VirtioConsoleDriver::new(registers)?;
		drv.init_dev(handlers, Some(irq))
			.map_err(VirtioError::ConsoleDriver)?;
		drv.caps_coll.com_cfg.print_information();
		Ok(drv)
	}
}
