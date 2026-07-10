use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::entropy::{EntropyDevCfg, RxQueue, VirtioEntropyDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::{ComCfg, NotifCfg};

impl VirtioEntropyDriver {
	pub fn new(
		dev_id: u16,
		mut registers: VolatileRef<'static, DeviceRegisters>,
	) -> Result<VirtioEntropyDriver, VirtioError> {
		let dev_cfg = EntropyDevCfg {
			dev_id,
			features: virtio::entropy::F::empty(),
		};
		let notif_cfg = NotifCfg::new(registers.borrow_mut());

		Ok(VirtioEntropyDriver {
			dev_cfg,
			com_cfg: ComCfg::new(registers),
			notif_cfg,
			recv_vq: RxQueue::new(),
		})
	}

	pub fn init(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
	) -> Result<VirtioEntropyDriver, VirtioError> {
		let mut drv = VirtioEntropyDriver::new(dev_id, registers)?;
		drv.init_dev().map_err(VirtioError::EntropyDriver)?;
		drv.com_cfg.print_information();
		Ok(drv)
	}
}
