use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::map_caps;
use crate::drivers::vsock::{EventQueue, RxQueue, TxQueue, VirtioVsockDriver, VsockDevCfg};
use crate::drivers::{InterruptHandlerMap, InterruptLine};

// Backend-dependent interface for Virtio vsock driver
impl VirtioVsockDriver {
	pub fn new(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
	) -> Result<VirtioVsockDriver, VirtioError> {
		let (caps_coll, dev_cfg_raw) = map_caps(registers);
		let dev_cfg = VsockDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::vsock::F::empty(),
		};

		Ok(VirtioVsockDriver {
			dev_cfg,
			caps_coll,
			event_vq: EventQueue::new(),
			recv_vq: RxQueue::new(),
			send_vq: TxQueue::new(),
		})
	}

	/// Initializes virtio vsock device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioVsockDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioVsockDriver, VirtioError> {
		let mut drv = VirtioVsockDriver::new(dev_id, registers)?;
		drv.init_dev(handlers, Some(irq))
			.map_err(VirtioError::VsockDriver)?;
		drv.caps_coll.com_cfg.print_information();
		Ok(drv)
	}
}
