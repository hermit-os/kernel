use smoltcp::phy::ChecksumCapabilities;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::drivers::InterruptLine;
use crate::drivers::net::virtio::{Init, NetDevCfg, Uninit, VirtioNetDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver<Uninit> {
	pub fn new(
		dev_id: u16,
		mut registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<Self, VirtioError> {
		let dev_cfg_raw: &'static virtio::net::Config = unsafe {
			&*registers
				.borrow_mut()
				.as_mut_ptr()
				.config()
				.as_raw_ptr()
				.cast::<virtio::net::Config>()
				.as_ptr()
		};
		let dev_cfg_raw = VolatileRef::from_ref(dev_cfg_raw);
		let dev_cfg = NetDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::net::F::empty(),
		};
		let isr_stat = IsrStatus::new(registers.borrow_mut());
		let notif_cfg = NotifCfg::new(registers.borrow_mut());

		Ok(VirtioNetDriver {
			dev_cfg,
			com_cfg: ComCfg::new(registers),
			isr_stat,
			notif_cfg,
			inner: Uninit,
			num_vqs: 0,
			irq: Some(irq),
			checksums: ChecksumCapabilities::default(),
		})
	}

	/// Initializes virtio network device by mapping configuration layout to
	/// respective structs (configuration structs are:
	///
	/// Returns a driver instance of
	/// [VirtioNetDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
		irq: InterruptLine,
	) -> Result<VirtioNetDriver<Init>, VirtioError> {
		let drv = VirtioNetDriver::new(dev_id, registers, irq)?;
		let mut drv = drv.init_dev().map_err(VirtioError::NetDriver)?;
		drv.print_information();
		Ok(drv)
	}
}

impl VirtioNetDriver<Init> {
	pub fn print_information(&mut self) {
		self.com_cfg.print_information();
		if self.dev_status() == virtio::net::S::LINK_UP {
			info!("The link of the network device is up!");
		}
	}
}
