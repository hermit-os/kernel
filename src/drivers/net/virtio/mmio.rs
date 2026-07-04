use smoltcp::phy::ChecksumCapabilities;
use virtio::mmio::DeviceRegisters;
use volatile::VolatileRef;

use crate::drivers::net::virtio::{Init, NetDevCfg, Uninit, VirtioNetDriver};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::mmio::map_caps;
use crate::drivers::{InterruptHandlerMap, InterruptLine};

// Backend-dependent interface for Virtio network driver
impl VirtioNetDriver<Uninit> {
	pub fn new(
		dev_id: u16,
		registers: VolatileRef<'static, DeviceRegisters>,
	) -> Result<Self, VirtioError> {
		let (caps_coll, dev_cfg_raw) = map_caps(registers);
		let dev_cfg = NetDevCfg {
			raw: dev_cfg_raw,
			dev_id,
			features: virtio::net::F::empty(),
		};

		Ok(VirtioNetDriver {
			dev_cfg,
			caps_coll,
			inner: Uninit,
			num_vqs: 0,
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
		handlers: &mut InterruptHandlerMap,
	) -> Result<VirtioNetDriver<Init>, VirtioError> {
		let drv = VirtioNetDriver::new(dev_id, registers)?;
		let mut drv = drv
			.init_dev(handlers, Some(irq))
			.map_err(VirtioError::NetDriver)?;
		drv.print_information();
		Ok(drv)
	}
}

impl VirtioNetDriver<Init> {
	pub fn print_information(&mut self) {
		self.caps_coll.com_cfg.print_information();
		if self.dev_status() == virtio::net::S::LINK_UP {
			info!("The link of the network device is up!");
		}
	}
}
