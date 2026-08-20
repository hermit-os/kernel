//! A virtio-blk driver.
//!
//! For details on the device, see [Block Device].
//!
//! [Block Device]: https://docs.oasis-open.org/virtio/virtio/v1.4/cs01/virtio-v1.4-cs01.html#x1-3120002

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::task::Poll;

use hermit_sync::InterruptTicketMutex;
use smallvec::SmallVec;
use virtio::blk::{ConfigVolatileFieldAccess, RequestHeader, RequestType, Status};
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use crate::arch::kernel::core_local::core_id;
use crate::config::VIRTIO_MAX_QUEUE_SIZE;
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_block_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_block_driver;
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::{InterruptCapability, UniCapsColl};
use crate::drivers::virtio::virtqueue::error::VirtqError;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, VirtQueue, Virtq,
};
use crate::drivers::{Driver, InterruptHandlerMap, InterruptLine};
use crate::errno::Errno;
use crate::mm::device_alloc::DeviceAlloc;

/// The block device driver, if the system has one.
pub(crate) fn driver() -> Option<&'static VirtioBlkDriver> {
	get_block_driver()
}

/// Runs `f` on the block device driver, if the system has one.
pub(crate) fn with_driver<T>(f: impl FnOnce(&VirtioBlkDriver) -> T) -> Option<T> {
	get_block_driver().map(f)
}

/// The unit in which the device addresses its storage.
///
/// This is fixed by the specification and unrelated to
/// `VirtioBlkDriver::block_size`, which merely reports the optimal I/O size.
pub(crate) const SECTOR_SIZE: usize = 512;

/// The number of cores that may ever issue a request.
///
/// Deliberately not `get_processor_count`: that one counts the cores which
/// have finished starting, and at driver initialization time only the boot
/// processor has, so sizing the queues by it would always settle on one.
fn possible_cores() -> usize {
	#[cfg(feature = "smp")]
	let count = crate::arch::kernel::get_possible_cpus();
	#[cfg(not(feature = "smp"))]
	let count = 1;

	usize::try_from(count).unwrap()
}

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
	/// The request virtqueues, at least one. `Self::vq` picks one per
	/// requesting core.
	///
	/// The lock is asynchronous because it is held across the wait for the
	/// device: a second task on the same queue then yields instead of
	/// spinning, and the interrupt handler never takes it, so nothing here
	/// needs interrupts masked.
	vqs: Vec<async_lock::Mutex<VirtQueue>>,
	/// Reached only by `Self::handle_interrupt` during operation; everything
	/// else in here belongs to initialization.
	caps: InterruptTicketMutex<UniCapsColl>,
	/// Negotiated feature set, fixed once the device is live.
	features: virtio::blk::F,
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
	pub async fn read(&self, sector: usize, buf: &mut [u8]) -> Result<(), Errno> {
		let len = self.checked_len(sector, buf.len())?;

		let send = single(BufferElem::Sized(Box::new_in(
			RequestHeader::new(RequestType::IN, sector.try_into().unwrap()),
			DeviceAlloc,
		)));
		let recv = SmallVec::from_buf([
			BufferElem::Vector(Vec::with_capacity_in(len, DeviceAlloc)),
			BufferElem::Sized(Box::<u8, _>::new_uninit_in(DeviceAlloc)),
		]);

		let mut used = self.dispatch(send, recv).await?;

		let data = used.used_recv_buff.pop_front_vec().ok_or(Errno::Io)?;
		Self::check_status(&mut used)?;

		if data.len() != len {
			return Err(Errno::Io);
		}
		buf.copy_from_slice(&data);

		Ok(())
	}

	/// Writes `buf.len() / SECTOR_SIZE` sectors starting at `sector`.
	pub async fn write(&self, sector: usize, buf: &[u8]) -> Result<(), Errno> {
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

		let mut used = self.dispatch(send, recv).await?;
		Self::check_status(&mut used)
	}

	/// Asks the device to write out its cache.
	///
	/// Without `virtio::blk::F::FLUSH` this is a no-op. That feature is how a
	/// device announces that it may hold writes in a volatile cache, so one
	/// that withholds it has nothing left to write out once `dispatch`
	/// returned. Reporting an error here would fail an `fsync` whose data did
	/// reach the device.
	pub async fn flush(&self) -> Result<(), Errno> {
		if !self.features.contains(virtio::blk::F::FLUSH) {
			return Ok(());
		}

		let send = single(BufferElem::Sized(Box::new_in(
			RequestHeader::new(RequestType::FLUSH, 0),
			DeviceAlloc,
		)));
		let recv = single(BufferElem::Sized(Box::<u8, _>::new_uninit_in(DeviceAlloc)));

		let mut used = self.dispatch(send, recv).await?;
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

	/// Picks the virtqueue for a request issued on the current core.
	///
	/// Sharing one queue between cores would make them contend for its lock;
	/// mapping each core to its own keeps a request on the core that issued
	/// it. The modulo covers a device that offers fewer queues than the system
	/// has cores — those cores then share a queue, which costs contention but
	/// stays correct.
	fn vq(&self) -> &async_lock::Mutex<VirtQueue> {
		let index = usize::try_from(core_id()).unwrap() % self.vqs.len();

		&self.vqs[index]
	}

	/// Sends a request and waits for the device to complete it.
	///
	/// The queue lock is held for the whole exchange, which is what makes the
	/// `try_recv` below unambiguous: the completion it finds can only be the
	/// one this call put in. A second task wanting the same queue waits on the
	/// lock, and because that lock is asynchronous it yields the core rather
	/// than spinning on it.
	async fn dispatch(
		&self,
		send: SmallVec<[BufferElem; 2]>,
		recv: SmallVec<[BufferElem; 2]>,
	) -> Result<crate::drivers::virtio::virtqueue::UsedBufferToken, Errno> {
		let tkn = AvailBufferToken::new(send, recv).map_err(|_| Errno::Io)?;
		let mut vq = self.vq().lock().await;

		vq.dispatch(tkn, false, BufferType::Direct)
			.map_err(|_| Errno::Io)?;

		core::future::poll_fn(|cx| match vq.try_recv() {
			Ok(used) => Poll::Ready(Ok(used)),
			Err(VirtqError::NoNewUsed) => {
				// The device is still working. Ask to be polled again rather
				// than spinning here: `block_on` runs the executor's other
				// tasks between polls and puts the task to sleep once its
				// backoff is exhausted, and interrupts stay unmasked
				// throughout.
				cx.waker().wake_by_ref();
				Poll::Pending
			}
			Err(_) => Poll::Ready(Err(Errno::Io)),
		})
		.await
	}

	/// Acknowledges a device interrupt.
	///
	/// The driver completes requests by polling, so nothing has to be done
	/// with the information — but the ISR status register *must* be read.
	pub fn handle_interrupt(&self) {
		let mut caps = self.caps.lock();

		#[cfg_attr(
			not(all(feature = "pci", target_arch = "x86_64")),
			expect(irrefutable_let_patterns)
		)]
		let InterruptCapability::IsrStatus(isr_stat) = &mut caps.int_cap else {
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
		.union(virtio::blk::F::FLUSH)
		.union(virtio::blk::F::MQ);

	fn init_dev(
		(mut caps_coll, dev_cfg_raw): (UniCapsColl, VolatileRef<'static, Self::Config, ReadOnly>),
		handlers: &mut InterruptHandlerMap,
		irq: Option<InterruptLine>,
	) -> Result<Self, (VirtioError, UniCapsColl)> {
		let mut queues = Vec::new();

		let dev_cfg = match caps_coll.init_caps::<Self>(dev_cfg_raw, |caps_coll, dev_cfg| {
			// `num_queues` only carries a meaning once MQ was negotiated; a
			// device without it exposes exactly one request queue.
			let features: virtio::blk::F = dev_cfg.features;
			let offered = if features.contains(virtio::blk::F::MQ) {
				dev_cfg.raw.as_ptr().num_queues().read().to_ne()
			} else {
				1
			};

			// More queues than cores would stay idle, since a queue is chosen
			// by the core issuing the request. A device is free to offer up to
			// 65535 of them, so the clamp also bounds the memory the rings
			// take.
			let count = usize::from(offered.max(1)).min(possible_cores());

			for index in 0..count {
				queues.push(VirtQueue::Split(
					SplitVq::new(
						&mut caps_coll.com_cfg,
						&caps_coll.notif_cfg,
						VIRTIO_MAX_QUEUE_SIZE,
						u16::try_from(index).unwrap(),
						virtio::F::from(dev_cfg.features),
					)
					.unwrap(),
				));
			}

			// The driver never waits for interrupts, but the ISR status still
			// has to be acknowledged on every interrupt
			match &mut caps_coll.int_cap {
				InterruptCapability::IsrStatus(_) => {
					let irq = irq.unwrap();
					handlers.entry(irq).or_default().push_back(|| {
						if let Some(driver) = get_block_driver() {
							driver.handle_interrupt();
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
								driver.handle_interrupt();
							};
						},
						iter::empty::<(iter::Empty<_>, _)>(),
						0..u16::try_from(queues.len()).unwrap(),
					);
				}
			}

			Ok(())
		}) {
			Ok(dev_cfg) => dev_cfg,
			Err(err) => return Err((err, caps_coll)),
		};

		// Requests are awaited by polling, so the device never has to notify us.
		for vq in &mut queues {
			vq.disable_notifs();
		}

		let cfg = dev_cfg.raw.as_ptr();
		let features: virtio::blk::F = dev_cfg.features;

		let capacity = cfg.capacity().read().to_ne();
		let blk_size = if features.contains(virtio::blk::F::BLK_SIZE) {
			cfg.blk_size().read().to_ne()
		} else {
			u32::try_from(SECTOR_SIZE).unwrap()
		};
		let read_only = features.contains(virtio::blk::F::RO);
		let vqs: Vec<_> = queues.into_iter().map(async_lock::Mutex::new).collect();

		info!(
			"virtio-blk: {capacity} sectors ({} MiB), block size {blk_size}, {} request queue(s){}",
			capacity * u64::try_from(SECTOR_SIZE).unwrap() / (1024 * 1024),
			vqs.len(),
			if read_only { ", read-only" } else { "" }
		);

		Ok(Self {
			vqs,
			caps: InterruptTicketMutex::new(caps_coll),
			features,
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
