//! A virtio-blk driver.
//!
//! For details on the device, see [Block Device].
//!
//! [Block Device]: https://docs.oasis-open.org/virtio/virtio/v1.4/cs01/virtio-v1.4-cs01.html#x1-3120002

use alloc::boxed::Box;
use alloc::vec::Vec;

use smallvec::SmallVec;
use virtio::blk::{ConfigVolatileFieldAccess, RequestHeader, RequestType, Status};
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use crate::config::VIRTIO_MAX_QUEUE_SIZE;
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_block_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_block_driver;
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::{InterruptCapability, UniCapsColl};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, VirtQueue, Virtq,
};
use crate::drivers::{Driver, InterruptHandlerMap, InterruptLine};
use crate::errno::Errno;
use crate::mm::device_alloc::DeviceAlloc;

/// Runs `f` on the block device driver, if the system has one.
pub(crate) fn with_driver<T>(f: impl FnOnce(&mut VirtioBlkDriver) -> T) -> Option<T> {
	get_block_driver().map(|drv| f(&mut drv.lock()))
}

/// The unit in which the device addresses its storage.
///
/// This is fixed by the specification and unrelated to
/// `VirtioBlkDriver::block_size`, which merely reports the optimal I/O size.
pub(crate) const SECTOR_SIZE: usize = 512;

/// Wraps a single buffer element in the [`SmallVec`] the virtqueue expects.
fn single(elem: BufferElem) -> SmallVec<[BufferElem; 2]> {
	let mut vec = SmallVec::new();
	vec.push(elem);
	vec
}

/// Driver for a virtio block device.
///
/// Requests are dispatched one at a time and awaited by polling the used ring,
/// so the driver needs no interrupt handler and keeps device notifications
/// disabled.
pub(crate) struct VirtioBlkDriver {
	pub(super) dev_cfg: super::virtio::DevCfg<Self>,
	pub(super) caps_coll: UniCapsColl,
	vq: VirtQueue,
	capacity: usize,
	blk_size: u32,
	read_only: bool,
}

impl VirtioBlkDriver {
	/// The capacity of the device in `SECTOR_SIZE`-byte sectors.
	pub fn capacity(&self) -> usize {
		self.capacity
	}

	/// The optimal I/O size in bytes, or `SECTOR_SIZE` if the device does not
	/// report one.
	#[allow(dead_code)]
	pub fn block_size(&self) -> u32 {
		self.blk_size
	}

	/// Whether the device refuses writes.
	#[allow(dead_code)]
	pub fn is_read_only(&self) -> bool {
		self.read_only
	}

	/// Reads `buf.len() / SECTOR_SIZE` sectors starting at `sector`.
	pub fn read(&mut self, sector: usize, buf: &mut [u8]) -> Result<(), Errno> {
		let len = self.checked_len(sector, buf.len())?;

		let send = single(BufferElem::Sized(Box::new_in(
			RequestHeader::new(RequestType::IN, sector.try_into().unwrap()),
			DeviceAlloc,
		)));
		let recv = SmallVec::from_buf([
			BufferElem::Vector(Vec::with_capacity_in(len, DeviceAlloc)),
			BufferElem::Sized(Box::<u8, _>::new_uninit_in(DeviceAlloc)),
		]);

		let mut used = self.dispatch(send, recv)?;

		let data = used.used_recv_buff.pop_front_vec().ok_or(Errno::Io)?;
		Self::check_status(&mut used)?;

		if data.len() != len {
			return Err(Errno::Io);
		}
		buf.copy_from_slice(&data);

		Ok(())
	}

	/// Writes `buf.len() / SECTOR_SIZE` sectors starting at `sector`.
	pub fn write(&mut self, sector: usize, buf: &[u8]) -> Result<(), Errno> {
		let _len = self.checked_len(sector, buf.len())?;

		if self.read_only {
			return Err(Errno::Rofs);
		}

		let mut data = Vec::with_capacity_in(buf.len(), DeviceAlloc);
		data.extend_from_slice(buf);

		let send = SmallVec::from_buf([
			BufferElem::Sized(Box::new_in(
				RequestHeader::new(RequestType::OUT, sector.try_into().unwrap()),
				DeviceAlloc,
			)),
			BufferElem::Vector(data),
		]);
		let recv = single(BufferElem::Sized(Box::<u8, _>::new_uninit_in(DeviceAlloc)));

		let mut used = self.dispatch(send, recv)?;
		Self::check_status(&mut used)
	}

	/// Asks the device to write out its cache.
	///
	/// Returns `Errno::Nosys` if the device did not negotiate
	/// [`feature::FLUSH`].
	pub fn flush(&mut self) -> Result<(), Errno> {
		if !self.dev_cfg.features.contains(virtio::blk::F::FLUSH) {
			return Err(Errno::Nosys);
		}

		let send = single(BufferElem::Sized(Box::new_in(
			RequestHeader::new(RequestType::FLUSH, 0),
			DeviceAlloc,
		)));
		let recv = single(BufferElem::Sized(Box::<u8, _>::new_uninit_in(DeviceAlloc)));

		let mut used = self.dispatch(send, recv)?;
		Self::check_status(&mut used)
	}

	/// Validates a transfer against the sector size and the device capacity and
	/// returns its length in bytes.
	fn checked_len(&self, sector: usize, len: usize) -> Result<usize, Errno> {
		if len == 0 || !len.is_multiple_of(SECTOR_SIZE) {
			return Err(Errno::Inval);
		}

		let sectors = len / SECTOR_SIZE;
		if sector
			.checked_add(sectors)
			.is_none_or(|end| end > self.capacity)
		{
			return Err(Errno::Inval);
		}

		Ok(len)
	}

	fn dispatch(
		&mut self,
		send: SmallVec<[BufferElem; 2]>,
		recv: SmallVec<[BufferElem; 2]>,
	) -> Result<crate::drivers::virtio::virtqueue::UsedBufferToken, Errno> {
		let tkn = AvailBufferToken::new(send, recv).map_err(|_| Errno::Io)?;
		self.vq
			.dispatch_blocking(tkn, BufferType::Direct)
			.map_err(|_| Errno::Io)
	}

	/// Acknowledges a device interrupt.
	///
	/// The driver completes requests by polling, so nothing has to be done
	/// with the information — but the ISR status register *must* be read.
	pub fn handle_interrupt(&mut self) {
		#[cfg_attr(
			not(all(feature = "pci", target_arch = "x86_64")),
			expect(irrefutable_let_patterns)
		)]
		let InterruptCapability::IsrStatus(isr_stat) = &mut self.caps_coll.int_cap else {
			panic!("MSI-X vectors should be configured to the interrupt type-specific handlers.")
		};

		let _ = isr_stat.acknowledge();
	}

	/// Pops the trailing status byte and maps it onto an `Errno`.
	fn check_status(
		used: &mut crate::drivers::virtio::virtqueue::UsedBufferToken,
	) -> Result<(), Errno> {
		// The byte is taken as a `u8` rather than as a `Status`, because the
		// device is free to write any value.
		let status = unsafe { used.used_recv_buff.pop_front_downcast::<u8>() };
		let Some(status) = status.as_deref().copied() else {
			// The device did not write the status byte at all.
			return Err(Errno::Io);
		};

		if status == u8::from(Status::OK) {
			Ok(())
		} else if status == u8::from(Status::UNSUPP) {
			Err(Errno::Nosys)
		} else {
			// Either VIRTIO_BLK_S_IOERR or a status this driver does not know.
			Err(Errno::Io)
		}
	}
}

impl Driver for VirtioBlkDriver {
	fn get_name() -> &'static str {
		"virtio-blk"
	}
}

impl super::virtio::VirtioDriver for VirtioBlkDriver {
	type Config = virtio::blk::Config;
	type Error = error::VirtioBlkError;
	type DeviceFeatures = virtio::blk::F;

	const MINIMAL_FEATURES: Self::DeviceFeatures = virtio::blk::F::VERSION_1;
	const OPTIONAL_FEATURES: Self::DeviceFeatures = virtio::blk::F::SIZE_MAX
		.union(virtio::blk::F::SEG_MAX)
		.union(virtio::blk::F::RO)
		.union(virtio::blk::F::BLK_SIZE)
		.union(virtio::blk::F::FLUSH);

	fn init_dev(
		(mut caps_coll, dev_cfg_raw): (UniCapsColl, VolatileRef<'static, Self::Config, ReadOnly>),
		handlers: &mut InterruptHandlerMap,
		irq: Option<InterruptLine>,
	) -> Result<Self, (VirtioError, UniCapsColl)> {
		let mut vq = None;

		let dev_cfg = match caps_coll.init_caps(dev_cfg_raw, |caps_coll, dev_cfg| {
			// The device exposes one request queue unless VIRTIO_BLK_F_MQ is
			// negotiated, which this driver does not ask for.
			vq = Some(VirtQueue::Split(
				SplitVq::new(
					&mut caps_coll.com_cfg,
					&caps_coll.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					0,
					virtio::F::from(dev_cfg.features),
				)
				.unwrap(),
			));

			// The driver never waits for interrupts, but the ISR status still
			// has to be acknowledged on every interrupt
			match &mut caps_coll.int_cap {
				InterruptCapability::IsrStatus(_) => {
					let irq = irq.unwrap();
					handlers.entry(irq).or_default().push_back(|| {
						if let Some(driver) = get_block_driver() {
							driver.lock().handle_interrupt();
						};
					});
					crate::arch::kernel::interrupts::add_irq_name(irq, "virtio");
					info!("Virtio interrupt handler at line {irq}");
				}
				#[cfg(all(feature = "pci", target_arch = "x86_64"))]
				InterruptCapability::Msix(msix_table) => {
					use core::iter;

					caps_coll.com_cfg.register_msix_vectors(
						msix_table,
						handlers,
						|| {
							if let Some(driver) = get_block_driver() {
								driver.lock().handle_interrupt();
							};
						},
						iter::empty::<(iter::Empty<_>, _)>(),
						0..1,
					);
				}
			}

			Ok(())
		}) {
			Ok(dev_cfg) => dev_cfg,
			Err(err) => return Err((err, caps_coll)),
		};

		let mut vq = vq.unwrap();
		// Requests are awaited by polling, so the device never has to notify us.
		vq.disable_notifs();

		let cfg = dev_cfg.raw.as_ptr();
		let features: virtio::blk::F = dev_cfg.features;

		let capacity = cfg.capacity().read().to_ne();
		let blk_size = if features.contains(virtio::blk::F::BLK_SIZE) {
			cfg.blk_size().read().to_ne()
		} else {
			u32::try_from(SECTOR_SIZE).unwrap()
		};
		let read_only = features.contains(virtio::blk::F::RO);

		info!(
			"virtio-blk: {capacity} sectors ({} MiB), block size {blk_size}{}",
			capacity * u64::try_from(SECTOR_SIZE).unwrap() / (1024 * 1024),
			if read_only { ", read-only" } else { "" }
		);

		Ok(Self {
			dev_cfg,
			caps_coll,
			vq,
			capacity: capacity.try_into().unwrap(),
			blk_size,
			read_only,
		})
	}

	#[cfg(feature = "pci")]
	fn no_dev_cfg_err(dev_id: u16) -> Self::Error {
		error::VirtioBlkError::NoDevCfg(dev_id)
	}
}

/// Error module of the virtio block driver.
pub mod error {
	use thiserror::Error;

	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioBlkError {
		#[cfg(feature = "pci")]
		#[error(
			"Virtio block device driver failed, for device {0:x}, due to a missing or malformed device config!"
		)]
		NoDevCfg(u16),
	}
}
