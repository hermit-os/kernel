use alloc::vec::Vec;

use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::fs::{FsDevCfg, VirtioFsDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::map_caps;
use crate::drivers::{InterruptHandlerMap, InterruptLine};

// Backend-dependent interface for Virtio fs driver
impl VirtioFsDriver {
	pub fn new(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
	) -> Result<VirtioFsDriver, VirtioError> {
		let (caps_coll, dev_cfg_raw) = map_caps(registers);
		let dev_cfg = FsDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::fs::F::empty(),
		};

		Ok(VirtioFsDriver {
			dev_cfg,
			caps_coll,
			vqueues: Vec::new(),
		})
	}

	/// Initializes virtio fs device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioFsDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioFsDriver, VirtioError> {
		let mut drv = VirtioFsDriver::new(dev_id, registers)?;
		drv.init_dev(handlers, Some(irq))
			.map_err(VirtioError::FsDriver)?;
		drv.caps_coll.com_cfg.print_information();
		Ok(drv)
	}
}
