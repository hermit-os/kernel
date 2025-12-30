use alloc::vec::Vec;

use virtio::fs::Config;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::drivers::InterruptLine;
use crate::drivers::fs::{FsDevCfg, VirtioFsDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};

// Backend-dependent interface for Virtio fs driver
impl VirtioFsDriver {
	pub fn new(
		dev_id: u16,
		mut registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<VirtioFsDriver, VirtioError> {
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
		let dev_cfg = FsDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::fs::F::empty(),
		};
		let isr_stat = IsrStatus::new(registers.borrow_mut());
		let notif_cfg = NotifCfg::new(registers.borrow_mut());

		Ok(VirtioFsDriver {
			dev_cfg,
			com_cfg: ComCfg::new(registers),
			isr_stat,
			notif_cfg,
			vqueues: Vec::new(),
			irq,
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
	) -> Result<VirtioFsDriver, VirtioError> {
		let mut drv = VirtioFsDriver::new(dev_id, registers, irq)?;
		drv.init_dev().map_err(VirtioError::FsDriver)?;
		Ok(drv)
	}
}
