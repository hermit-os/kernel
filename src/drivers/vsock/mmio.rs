use virtio::vsock::Config;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::drivers::InterruptLine;
use crate::drivers::vsock::{EventQueue, RxQueue, TxQueue, VirtioVsockDriver, VsockDevCfg};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};

// Backend-dependent interface for Virtio vsock driver
impl VirtioVsockDriver {
	pub fn new(
		dev_id: u16,
		mut registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<VirtioVsockDriver, VirtioError> {
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
		let dev_cfg = VsockDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::vsock::F::empty(),
		};
		let isr_stat = IsrStatus::new(registers.borrow_mut());
		let notif_cfg = NotifCfg::new(registers.borrow_mut());

		Ok(VirtioVsockDriver {
			dev_cfg,
			com_cfg: ComCfg::new(registers),
			isr_stat,
			notif_cfg,
			irq,
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
	) -> Result<VirtioVsockDriver, VirtioError> {
		let mut drv = VirtioVsockDriver::new(dev_id, registers, irq)?;
		drv.init_dev().map_err(VirtioError::VsockDriver)?;
		drv.com_cfg.print_information();
		Ok(drv)
	}
}
