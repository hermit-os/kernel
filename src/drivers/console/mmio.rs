//! A module containing a virtio console driver.
//!
//! The module contains ...

use virtio::console::Config;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::drivers::InterruptLine;
use crate::drivers::console::{ConsoleDevCfg, RxQueue, TxQueue, VirtioConsoleDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};

// Backend-dependent interface for Virtio console driver
impl VirtioConsoleDriver {
	pub fn new(
		dev_id: u16,
		mut registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<VirtioConsoleDriver, VirtioError> {
		let dev_cfg_raw: &'static Config = unsafe {
			&*registers
				.borrow_mut()
				.as_mut_ptr()
				.config()
				.as_raw_ptr()
				.cast::<Config>()
				.as_ptr()
		};
		let dev_cfg_raw = VolatileRef::from_ref(dev_cfg_raw);
		let dev_cfg = ConsoleDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::console::F::empty(),
		};
		let isr_stat = IsrStatus::new(registers.borrow_mut());
		let notif_cfg = NotifCfg::new(registers.borrow_mut());

		Ok(VirtioConsoleDriver {
			dev_cfg,
			com_cfg: ComCfg::new(registers, 1),
			isr_stat,
			notif_cfg,
			irq,
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
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<VirtioConsoleDriver, VirtioError> {
		let mut drv = VirtioConsoleDriver::new(dev_id, registers, irq)?;
		drv.init_dev()
			.map_err(|error_code| VirtioError::ConsoleDriver(error_code))?;
		Ok(drv)
	}
}
