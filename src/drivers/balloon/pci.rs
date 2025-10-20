use virtio::pci::IsrStatus;
use volatile::VolatileRef;

use super::{BalloonDevCfg, BalloonStorage, BalloonVq, VirtioBalloonDriver, VirtioBalloonError};
use crate::drivers::pci::PciDevice;
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::pci::{self as virtio_pci, PciCap, UniCapsColl};
use crate::pci::PciConfigRegion;

impl VirtioBalloonDriver {
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	fn map_cfg(cap: &PciCap) -> Option<BalloonDevCfg> {
		let dev_cfg = virtio_pci::map_dev_cfg::<virtio::balloon::Config>(cap)?;

		let dev_cfg = VolatileRef::from_mut_ref(dev_cfg);

		Some(BalloonDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: virtio::balloon::F::empty(),
		})
	}

	/// Instantiates a new [`VirtioBalloonDriver`] struct, by checking the available
	/// configuration structures and moving them into the struct.
	fn new(
		caps_coll: UniCapsColl,
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Self, VirtioBalloonError> {
		let device_id = device.device_id();

		let UniCapsColl {
			com_cfg,
			notif_cfg,
			isr_cfg,
			dev_cfg_list,
			..
		} = caps_coll;

		let Some(dev_cfg) = dev_cfg_list.iter().find_map(VirtioBalloonDriver::map_cfg) else {
			error!("<balloon:pci> No dev config. Aborting!");
			return Err(VirtioBalloonError::NoDevCfg { device_id });
		};

		Ok(VirtioBalloonDriver {
			dev_cfg,
			com_cfg,
			isr_stat: isr_cfg,
			notif_cfg,
			irq: device.get_irq().unwrap(),

			inflateq: BalloonVq::new(),
			deflateq: BalloonVq::new(),

			num_in_balloon: 0,
			num_pending_inflation: 0,
			num_pending_deflation: 0,
			num_targeted: 0,

			balloon_storage: BalloonStorage::new(),
			last_voluntary_inflate: 0,
		})
	}

	/// Initialize a new VIRTIO Traditional Memory Balloon device based on the given PCI device
	pub fn from_pci_device(
		device: &PciDevice<PciConfigRegion>,
	) -> Result<VirtioBalloonDriver, VirtioError> {
		let caps = virtio_pci::map_caps(device).inspect_err(|_| {
			error!("<balloon:pci> Mapping capabilities failed. Aborting!");
		})?;

		let mut driver = VirtioBalloonDriver::new(caps, device)
			.inspect_err(|_| {
				error!("<balloon:pci> Initializing new driver failed. Aborting!");
			})
			.map_err(VirtioError::BalloonDriver)?;

		driver
			.init_dev()
			.inspect_err(|_| driver.set_failed())
			.map_err(VirtioError::BalloonDriver)?;

		info!(
			"<balloon:pci> device with id {:x}, has been initialized by driver!",
			driver.get_dev_id()
		);

		Ok(driver)
	}

	pub fn handle_interrupt(&mut self) {
		let status = self.isr_stat.is_queue_interrupt();

		if status.contains(IsrStatus::DEVICE_CONFIGURATION_INTERRUPT) {
			debug!(
				"<balloon:pci> Received config interrupt, new config: {:?}",
				self.dev_cfg
			);
		}

		if status.contains(IsrStatus::QUEUE_INTERRUPT) {
			debug!("<balloon:pci> Received queue interrupt");
		}

		// TODO: wake tasks via wakers once introduced (currently every task just gets polled round-robin, always)

		self.isr_stat.acknowledge();
	}
}
