//! virtio-pci infrastructure.
//!
//! For details on the device, see [Virtio Over PCI Bus].
//! For details on the Rust definitions, see [`virtio::pci`].
//!
//! [Virtio Over PCI Bus]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-1150001

use alloc::vec::Vec;
use core::ptr::{self, NonNull};

use memory_addresses::PhysAddr;
use pci_types::capability::PciCapability;
use thiserror::Error;
use virtio::pci::{
	CapCfgType, CapData, CommonCfg, CommonCfgVolatileFieldAccess, CommonCfgVolatileWideFieldAccess,
	IsrStatus as IsrStatusRaw, NotificationData,
};
use virtio::{DeviceStatus, le16, le32};
use volatile::access::ReadOnly;
use volatile::{VolatilePtr, VolatileRef};

use crate::arch::kernel::pci::PciConfigRegion;
use crate::drivers::InterruptHandlerMap;
#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioConsoleDriver;
use crate::drivers::error::DriverError;
#[cfg(feature = "virtio-fs")]
use crate::drivers::fs::VirtioFsDriver;
#[cfg(all(
	not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
	not(feature = "rtl8139"),
	feature = "virtio-net",
))]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::pci::PciDevice;
use crate::drivers::pci::error::PciError;
#[cfg(all(feature = "pci", target_arch = "x86_64"))]
use crate::drivers::pci::msix;
#[cfg(target_arch = "x86_64")]
use crate::drivers::pci::msix::MsixTableVolatileElementAccess;
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::pci::PciBar as VirtioPciBar;
use crate::drivers::virtio::transport::{InterruptCapability, UniCapsColl};
use crate::drivers::virtio::{ControlRegisters, VirtioIdExt};
#[cfg(feature = "virtio-vsock")]
use crate::drivers::vsock::VirtioVsockDriver;

/// Virtio's PCI capabilities structure.
/// See Virtio specification v.1.1 - 4.1.4
///
/// Indicating: Where the capability field is mapped in memory and
/// Which id (sometimes also indicates priority for multiple
/// capabilities of same type) it holds.
///
/// This structure does NOT represent the structure in the standard,
/// as it is not directly mapped into address space from PCI device
/// configuration space.
/// Therefore the struct only contains necessary information to map
/// corresponding config type into address space.
#[derive(Clone)]
pub struct PciCap {
	bar: PciBar,
	cap: CapData,
}

impl PciCap {
	pub fn new(bar: PciBar, cap: CapData) -> Option<Self> {
		(bar.length >= cap.length.to_ne() + cap.offset.to_ne()).then_some(Self { bar, cap })
	}

	pub fn offset(&self) -> u64 {
		self.cap.offset.to_ne()
	}

	pub fn len(&self) -> u64 {
		self.cap.length.to_ne()
	}

	pub fn bar_addr(&self) -> u64 {
		self.bar.mem_addr
	}

	/// Maps a given device specific pci configuration structure and
	/// returns a volatile reference to the actual structure inside the PCI devices memory space.
	pub fn map_cap_cfg<T: CapCfg, A: volatile::access::Access>(
		&self,
	) -> Result<VolatileRef<'static, T, A>, CapCfgError> {
		if self.cap.cfg_type != T::TYPE {
			return Err(CapCfgError::WrongCfgType);
		}

		// Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
		if usize::try_from(self.len()).unwrap() < T::min_size() {
			return Err(CapCfgError::StructTooLarge);
		}

		let virt_addr_raw = self.bar_addr() + self.offset();
		let ptr = NonNull::new(ptr::with_exposed_provenance_mut::<T>(
			virt_addr_raw.try_into().unwrap(),
		))
		.unwrap();

		// Create mutable reference to the PCI structure in PCI memory
		let cap_cfg_raw = unsafe { VolatileRef::new(ptr) };

		Ok(cap_cfg_raw.restrict())
	}
}

pub(crate) trait CapCfg: Sized {
	const TYPE: CapCfgType;

	fn min_size() -> usize {
		size_of::<Self>()
	}
}

#[derive(Error, Debug)]
pub(crate) enum CapCfgError {
	#[error("wrong capability config id, mapping not possible")]
	WrongCfgType,
	#[error("structure too large to fit into PCI capability")]
	StructTooLarge,
}

impl CapCfg for CommonCfg {
	const TYPE: CapCfgType = CapCfgType::Common;

	fn min_size() -> usize {
		// `CommonCfg::queue_notify_data` and `CommonCfg::queue_reset` are optional.
		size_of::<Self>() - size_of::<[le16; 2]>()
	}
}

impl CapCfg for IsrStatusRaw {
	const TYPE: CapCfgType = CapCfgType::Isr;
}

impl CapCfg for virtio::console::Config {
	const TYPE: CapCfgType = CapCfgType::Device;
}

impl CapCfg for virtio::fs::Config {
	const TYPE: CapCfgType = CapCfgType::Device;
}

impl CapCfg for virtio::net::Config {
	const TYPE: CapCfgType = CapCfgType::Device;
}

impl CapCfg for virtio::vsock::Config {
	const TYPE: CapCfgType = CapCfgType::Device;
}

/// Wraps a [`CommonCfg`] in order to preserve
/// the original structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via
/// the structure.
pub struct ComCfg {
	/// References the raw structure in PCI memory space. Is static as
	/// long as the device is present, which is mandatory in order to let this code work.
	com_cfg: VolatileRef<'static, CommonCfg>,
}

// Private interface of ComCfg
impl ComCfg {
	fn new(raw: VolatileRef<'static, CommonCfg>) -> Self {
		ComCfg { com_cfg: raw }
	}
}

pub struct VqCfgHandler<'a> {
	vq_index: u16,
	raw: VolatileRef<'a, CommonCfg>,
}

#[cfg_attr(not(target_arch = "x86_64"), expect(unused))]
pub(crate) const NO_VECTOR: u16 = 0xffff;

impl VqCfgHandler<'_> {
	// TODO: Create type for queue selected invariant to get rid of `self.select_queue()` everywhere.
	fn select_queue(&mut self) {
		self.raw
			.as_mut_ptr()
			.queue_select()
			.write(self.vq_index.into());
	}

	/// Sets the size of a given virtqueue. In case the provided size exceeds the maximum allowed
	/// size, the size is set to this maximum instead. Else size is set to the provided value.
	///
	/// Returns the set size in form of a `u16`.
	pub fn set_vq_size(&mut self, max_size: u16) -> u16 {
		self.select_queue();
		let queue_size = self.raw.as_mut_ptr().queue_size();

		let dev_queue_size = queue_size.read().to_ne();
		if dev_queue_size >= max_size {
			queue_size.write(max_size.into());
			debug_assert_eq!(queue_size.read().to_ne(), max_size);
			max_size
		} else {
			dev_queue_size
		}
	}

	pub fn set_ring_addr(&mut self, addr: PhysAddr) {
		self.select_queue();
		self.raw
			.as_mut_ptr()
			.queue_desc()
			.write(addr.as_u64().into());
	}

	pub fn set_drv_ctrl_addr(&mut self, addr: PhysAddr) {
		self.select_queue();
		self.raw
			.as_mut_ptr()
			.queue_driver()
			.write(addr.as_u64().into());
	}

	pub fn set_dev_ctrl_addr(&mut self, addr: PhysAddr) {
		self.select_queue();
		self.raw
			.as_mut_ptr()
			.queue_device()
			.write(addr.as_u64().into());
	}

	#[cfg(target_arch = "x86_64")]
	fn set_queue_msix_vector(&mut self, index: u16) -> Result<(), ()> {
		self.select_queue();
		let queue_msix_vector = self.raw.as_mut_ptr().queue_msix_vector();
		queue_msix_vector.write(index.into());
		let index_read = u16::from(queue_msix_vector.read());
		if index_read == index {
			Ok(())
		} else if index_read == NO_VECTOR {
			Err(())
		} else {
			unreachable!()
		}
	}

	pub fn notif_off(&mut self) -> u16 {
		self.select_queue();
		self.raw.as_mut_ptr().queue_notify_off().read().to_ne()
	}

	pub fn enable_queue(&mut self) {
		self.select_queue();
		self.raw.as_mut_ptr().queue_enable().write(1.into());
	}
}

// Public Interface of ComCfg
impl ComCfg {
	pub fn control_registers(&mut self) -> impl ControlRegisters<'_> {
		self.com_cfg.as_mut_ptr()
	}

	/// Select a queue via an index. If queue does NOT exist returns `None`, else
	/// returns `Some(VqCfgHandler)`.
	///
	/// INFO: The queue size is automatically bounded by constant `src::config:VIRTIO_MAX_QUEUE_SIZE`.
	pub fn select_vq(&mut self, index: u16) -> Option<VqCfgHandler<'_>> {
		self.com_cfg.as_mut_ptr().queue_select().write(index.into());

		if self.com_cfg.as_mut_ptr().queue_size().read().to_ne() == 0 {
			return None;
		}

		Some(VqCfgHandler {
			vq_index: index,
			raw: self.com_cfg.borrow_mut(),
		})
	}

	#[allow(dead_code)]
	pub fn device_config_space(&self) -> VolatilePtr<'_, CommonCfg, ReadOnly> {
		self.com_cfg.as_ptr()
	}

	/// Resets the device status field to zero.
	pub fn reset_dev(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.device_status()
			.write(DeviceStatus::empty());
	}

	/// Sets the device status field to FAILED.
	/// A driver MUST NOT initialize and use the device any further after this.
	/// A driver MAY use the device again after a proper reset of the device.
	#[allow(dead_code)]
	pub fn set_failed(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.device_status()
			.write(DeviceStatus::FAILED);
	}

	/// Sets the ACKNOWLEDGE bit in the device status field. This indicates, the
	/// OS has notived the device
	pub fn ack_dev(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.device_status()
			.update(|s| s | DeviceStatus::ACKNOWLEDGE);
	}

	/// Sets the DRIVER bit in the device status field. This indicates, the OS
	/// know how to run this device.
	pub fn set_drv(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.device_status()
			.update(|s| s | DeviceStatus::DRIVER);
	}

	/// Sets the FEATURES_OK bit in the device status field.
	///
	/// Drivers MUST NOT accept new features after this step.
	pub fn features_ok(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.device_status()
			.update(|s| s | DeviceStatus::FEATURES_OK);
	}

	/// In order to correctly check feature negotiaten, this function
	/// MUST be called after [self.features_ok()](ComCfg::features_ok()) in order to check
	/// if features have been accepted by the device after negotiation.
	///
	/// Re-reads device status to ensure the FEATURES_OK bit is still set:
	/// otherwise, the device does not support our subset of features and the device is unusable.
	pub fn check_features(&self) -> bool {
		let status = self.com_cfg.as_ptr().device_status().read();
		status.contains(DeviceStatus::FEATURES_OK)
	}

	/// Sets the DRIVER_OK bit in the device status field.
	///
	/// After this call, the device is "live"!
	pub fn drv_ok(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.device_status()
			.update(|s| s | DeviceStatus::DRIVER_OK);
	}

	pub fn does_device_need_reset(&self) -> bool {
		let status = self.com_cfg.as_ptr().device_status().read();
		status.contains(DeviceStatus::DEVICE_NEEDS_RESET)
	}
}

#[cfg(target_arch = "x86_64")]
impl ComCfg {
	fn set_config_msix_vector(&mut self, index: u16) -> Result<(), ()> {
		let config_msix_vector = self.com_cfg.as_mut_ptr().config_msix_vector();
		config_msix_vector.write(index.into());
		let index_read = u16::from(config_msix_vector.read());
		if index_read == index {
			Ok(())
		} else if index_read == NO_VECTOR {
			Err(())
		} else {
			unreachable!()
		}
	}

	pub(crate) fn register_msix_vectors(
		&mut self,
		msix_table: &mut VolatileRef<'_, [msix::TableEntry]>,
		handlers: &mut InterruptHandlerMap,
		config_handler: fn(),
		queue_handlers: impl ExactSizeIterator<Item = (impl IntoIterator<Item = u16>, fn())>,
		handlerless_queues: impl IntoIterator<Item = u16>,
	) {
		// One for the device config irq.
		let needed_irqs = 1 + queue_handlers.len();
		// We will need to map the IRQ number to the vector number by adding 32.
		const IRQ_RANGE: core::ops::RangeInclusive<u8> = 0..=(msix::VECTOR_MAX - 32);
		let mut free_irqs = IRQ_RANGE
			.filter(|v| !handlers.contains_key(v))
			// If we do not have enough free IRQs, fall back to using any
			// IRQ in the valid range.
			.chain(IRQ_RANGE)
			.take(needed_irqs)
			.collect::<Vec<_>>()
			.into_iter();

		const TABLE_CONFIG_INDEX: u16 = 0;
		let config_irq = free_irqs.next().unwrap();
		handlers
			.entry(config_irq)
			.or_default()
			.push_back(config_handler);
		msix_table.configure(TABLE_CONFIG_INDEX, config_irq + 32);
		self.set_config_msix_vector(TABLE_CONFIG_INDEX).unwrap();
		crate::arch::kernel::interrupts::add_irq_name(config_irq, "virtio config");
		info!("Virtio config interrupt handler at line {config_irq}");

		for (((queues, handler), irq), table_queue_index) in queue_handlers.zip(free_irqs).zip(1..)
		{
			handlers.entry(irq).or_default().push_back(handler);
			msix_table.configure(table_queue_index, irq + 32);
			for i in queues {
				self.select_vq(i)
					.unwrap()
					.set_queue_msix_vector(table_queue_index)
					.unwrap();
			}
			crate::arch::kernel::interrupts::add_irq_name(irq, "virtio queue");
			info!("Virtio queue interrupt handler at line {irq}");
		}

		for i in handlerless_queues {
			self.select_vq(i)
				.unwrap()
				.set_queue_msix_vector(NO_VECTOR)
				.unwrap();
		}
	}
}

/// Notification Structure to handle virtqueue notification settings.
/// See Virtio specification v1.1 - 4.1.4.4
pub struct NotifCfg {
	/// Start addr, from where the notification addresses for the virtqueues are computed
	base_addr: u64,
	notify_off_multiplier: u32,
	/// defines the maximum size of the notification space, starting from base_addr.
	#[cfg(debug_assertions)]
	length: u64,
}

impl NotifCfg {
	fn new(cap: &PciCap) -> Option<Self> {
		let notify_off_multiplier = cap.cap.notify_off_multiplier?.to_ne();

		// define base memory address from which the actual Queue Notify address can be derived via
		// base_addr + queue_notify_off * notify_off_multiplier.
		//
		// Where queue_notify_off is taken from the respective common configuration struct.
		// See Virtio specification v1.1. - 4.1.4.4
		//
		// Base address here already includes offset!
		let base_addr = cap.bar.mem_addr + cap.offset();

		Some(NotifCfg {
			base_addr,
			notify_off_multiplier,
			#[cfg(debug_assertions)]
			length: cap.len(),
		})
	}

	pub fn notification_location(&self, vq_cfg_handler: &mut VqCfgHandler<'_>) -> *mut le32 {
		let addend = u32::from(vq_cfg_handler.notif_off()) * self.notify_off_multiplier;

		// TODO: This should be
		// cap.length >= queue_notify_off * notify_off_multiplier + 4
		// if VIRTIO_F_NOTIFICATION_DATA has been negotiated.
		// Knowing this here requires a larger refactoring.
		#[cfg(debug_assertions)]
		assert!(self.length >= u64::from(addend + 2));

		let addr = self.base_addr + u64::from(addend);
		ptr::with_exposed_provenance_mut(addr.try_into().unwrap())
	}
}

/// Control structure, allowing to notify a device via PCI bus.
/// Typically hold by a virtqueue.
pub struct NotifCtrl {
	/// Indicates if VIRTIO_F_NOTIFICATION_DATA has been negotiated
	f_notif_data: bool,
	/// Where to write notification
	notif_addr: *mut le32,
}

// FIXME: make `notif_addr` implement `Send` instead
unsafe impl Send for NotifCtrl {}

impl NotifCtrl {
	/// Returns a new controller. By default MSI-X capabilities and VIRTIO_F_NOTIFICATION_DATA
	/// are disabled.
	pub fn new(notif_addr: *mut le32) -> Self {
		NotifCtrl {
			f_notif_data: false,
			notif_addr,
		}
	}

	/// Enables VIRTIO_F_NOTIFICATION_DATA. This changes which data is provided to the device. ONLY a good idea if Feature has been negotiated.
	pub fn enable_notif_data(&mut self) {
		self.f_notif_data = true;
	}

	pub fn notify_dev(&self, data: NotificationData) {
		// See Virtio specification v.1.1. - 4.1.5.2
		// Depending in the feature negotiation, we write either only the
		// virtqueue index or the index and the next position inside the queue.

		if self.f_notif_data {
			unsafe {
				self.notif_addr.write_volatile(data.into_bits());
			}
		} else {
			unsafe {
				self.notif_addr
					.cast::<le16>()
					.write_volatile(data.vqn().into());
			}
		}
	}
}

/// Wraps a [IsrStatusRaw] in order to preserve
/// the original structure and allow interaction with the device via
/// the structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via
/// the structure.
pub struct IsrStatus {
	/// References the raw structure in PCI memory space. Is static as
	/// long as the device is present, which is mandatory in order to let this code work.
	isr_stat: VolatileRef<'static, IsrStatusRaw>,
}

impl IsrStatus {
	fn new(raw: VolatileRef<'static, IsrStatusRaw>) -> Self {
		IsrStatus { isr_stat: raw }
	}

	pub fn acknowledge(&mut self) -> IsrStatusRaw {
		// Driver read of ISR status causes the device to de-assert an interrupt.
		// VIRTIO spec. v1.4 sec. 4.1.4.5
		self.isr_stat.as_ptr().read()
	}
}

/// PciBar stores the virtual memory address and associated length of memory space
/// a PCI device's physical memory indicated by the device's BAR has been mapped to.
//
// Currently all fields are public as the struct is instantiated in the drivers::virtio::env module
#[derive(Copy, Clone, Debug)]
pub struct PciBar {
	mem_addr: u64,
	length: u64,
}

impl PciBar {
	pub fn new(mem_addr: u64, length: u64) -> Self {
		PciBar { mem_addr, length }
	}
}

/// The return value contains a vector holding
/// [PciCap] objects, to allow the driver to map its
/// device specific configurations independently.
pub(crate) fn map_caps(
	device: &PciDevice<PciConfigRegion>,
) -> Result<(UniCapsColl, Vec<PciCap>), VirtioError> {
	let device_id = device.device_id();

	// In case caplist pointer is not used, abort as it is essential
	if !device.status().has_capability_list() {
		error!("Found virtio device without capability list. Aborting!");
		return Err(VirtioError::FromPci(PciError::NoCapPtr(device_id)));
	}

	let mut com_cfg = None;
	let mut notif_cfg = None;
	let mut isr_cfg = None;
	let mut dev_cfg_list = Vec::new();
	#[cfg(target_arch = "x86_64")]
	let mut msix_table = None;

	// Reads all PCI capabilities, starting at the capabilities list pointer from the
	// PCI device.
	//
	// Maps ONLY Virtio specific capabilities and the MSI-X capability , which allow to locate the actual capability
	// structures inside the memory areas, indicated by the BaseAddressRegisters (BAR's).
	for capability in device.capabilities().unwrap() {
		match capability {
			PciCapability::Vendor(addr) => {
				let cap = CapData::read(addr, device.access()).unwrap();
				if cap.cfg_type == CapCfgType::Pci {
					continue;
				}
				let slot = cap.bar;
				let Some((addr, size)) = device.memory_map_bar(slot, true) else {
					continue;
				};
				let Some(pci_cap) = PciCap::new(
					VirtioPciBar::new(addr.as_u64(), size.try_into().unwrap()),
					cap,
				) else {
					error!(
						"The capability of device {device_id:x} does not fit into memory specified by bar {slot:x}!",
					);
					continue;
				};
				match pci_cap.cap.cfg_type {
					CapCfgType::Common => {
						if com_cfg.is_none() {
							match pci_cap.map_cap_cfg() {
								Ok(cap) => com_cfg = Some(ComCfg::new(cap)),
								Err(err) => {
									error!("{err}");
									error!(
										"Common config capability of device {device_id:x} could not be mapped!"
									);
								}
							}
						}
					}
					CapCfgType::Notify => {
						if notif_cfg.is_none() {
							match NotifCfg::new(&pci_cap) {
								Some(notif) => notif_cfg = Some(notif),
								None => error!(
									"Notification config capability of device {device_id:x} could not be used!"
								),
							}
						}
					}
					CapCfgType::Isr => {
						let cond = isr_cfg.is_none();
						// We prefer MSI-X over ISR Status.
						#[cfg(target_arch = "x86_64")]
						let cond = cond && msix_table.is_none();
						if cond {
							match pci_cap.map_cap_cfg() {
								Ok(isr_stat) => isr_cfg = Some(IsrStatus::new(isr_stat)),
								Err(err) => {
									error!("{err}");
									error!(
										"ISR status config capability of device {device_id:x} could not be used!"
									);
								}
							}
						}
					}
					CapCfgType::SharedMemory => {
						let cap_id = pci_cap.cap.id;
						error!(
							"Shared Memory config capability with id {cap_id} of device {device_id:x} could not be used!"
						);
					}
					CapCfgType::Device => dev_cfg_list.push(pci_cap),
					_ => continue,
				}
			}
			// We can currently only make use of MSI-X on x86_64.
			#[cfg(target_arch = "x86_64")]
			PciCapability::MsiX(mut msix_capability) => {
				msix_capability.set_enabled(true, device.access());
				let (base_addr, _) = device
					.memory_map_bar(msix_capability.table_bar(), true)
					.unwrap();
				let table_ptr = NonNull::slice_from_raw_parts(
					NonNull::with_exposed_provenance(
						core::num::NonZero::new(
							base_addr.as_usize()
								+ usize::try_from(msix_capability.table_offset()).unwrap(),
						)
						.unwrap(),
					),
					msix_capability.table_size().into(),
				);
				msix_table = Some(unsafe { VolatileRef::new(table_ptr) });
			}
			// PCI's configuration space is allowed to hold other structures, which are not useful for us and are therefore ignored
			// in the following
			_ => continue,
		}
	}

	let isr_cfg = cfg_select! {
		target_arch = "x86_64" => msix_table.map(InterruptCapability::Msix),
		_ => None,
	}
	.or(isr_cfg.map(InterruptCapability::IsrStatus));

	match isr_cfg {
		Some(InterruptCapability::IsrStatus(_)) => {
			info!("The device will use legacy interrupts.");
		}
		#[cfg(target_arch = "x86_64")]
		Some(InterruptCapability::Msix(_)) => {
			info!("Found MSI-X capability. The device will use message signaled interrupts.");
		}
		_ => (),
	}

	Ok((
		UniCapsColl {
			com_cfg: com_cfg.ok_or(VirtioError::NoComCfg(device_id))?,
			notif_cfg: notif_cfg.ok_or(VirtioError::NoNotifCfg(device_id))?,
			int_cap: isr_cfg.ok_or(VirtioError::NoIsrCfg(device_id))?,
		},
		dev_cfg_list,
	))
}

/// Checks existing drivers for support of given device. Upon match, provides
/// driver with a [`PciDevice<PciConfigRegion>`] reference, allowing access to the capabilities
/// list of the given device through [map_caps].
#[cfg_attr(
	not(any(
		feature = "virtio-console",
		feature = "virtio-fs",
		all(
			not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
			not(feature = "rtl8139"),
			feature = "virtio-net",
		),
		feature = "virtio-vsock"
	)),
	expect(unused_variables)
)]
pub(crate) fn init_device(
	device: &PciDevice<PciConfigRegion>,
	handlers: &mut InterruptHandlerMap,
) -> Result<VirtioDriver, DriverError> {
	let device_id = device.device_id();

	if device_id < 0x1040 {
		warn!(
			"Legacy/transitional Virtio device, with id: {device_id:#x} is NOT supported, skipping!"
		);

		// Return Driver error inidacting device is not supported
		return Err(DriverError::InitVirtioDevFail(
			VirtioError::DevNotSupported(device_id),
		));
	}

	let id = virtio::Id::from(u8::try_from(device_id - 0x1040).unwrap());

	match id {
		#[cfg(feature = "virtio-console")]
		virtio::Id::Console => match VirtioConsoleDriver::init(device, handlers) {
			Ok(virt_console_drv) => {
				info!("Virtio console driver initialized.");
				Ok(VirtioDriver::Console(alloc::boxed::Box::new(
					virt_console_drv,
				)))
			}
			Err(virtio_error) => {
				error!("Virtio console driver could not be initialized with device: {device_id:x}");
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		#[cfg(feature = "virtio-fs")]
		virtio::Id::Fs => {
			// TODO: check subclass
			// TODO: proper error handling on driver creation fail
			match VirtioFsDriver::init(device, handlers) {
				Ok(virt_fs_drv) => {
					info!("Virtio filesystem driver initialized.");
					Ok(VirtioDriver::Fs(alloc::boxed::Box::new(virt_fs_drv)))
				}
				Err(virtio_error) => {
					error!(
						"Virtio filesystem driver could not be initialized with device: {device_id:x}"
					);
					Err(DriverError::InitVirtioDevFail(virtio_error))
				}
			}
		}
		#[cfg(all(
			not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
			not(feature = "rtl8139"),
			feature = "virtio-net",
		))]
		virtio::Id::Net => match VirtioNetDriver::init(device, handlers) {
			Ok(virt_net_drv) => {
				info!("Virtio network driver initialized.");
				Ok(VirtioDriver::Net(alloc::boxed::Box::new(virt_net_drv)))
			}
			Err(virtio_error) => {
				error!(
					"Virtio networkd driver could not be initialized with device: {device_id:x}"
				);
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		#[cfg(feature = "virtio-vsock")]
		virtio::Id::Vsock => match VirtioVsockDriver::init(device, handlers) {
			Ok(virt_sock_drv) => {
				info!("Virtio sock driver initialized.");
				Ok(VirtioDriver::Vsock(alloc::boxed::Box::new(virt_sock_drv)))
			}
			Err(virtio_error) => {
				error!("Virtio sock driver could not be initialized with device: {device_id:x}");
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		id => {
			if let Some(feature) = id.as_feature() {
				error!("Virtio driver {id:?} is currently not active.");
				error!("To use the device, recompile the kernel with the {feature} feature.");
			} else {
				error!("Virtio device {id:?} is not supported!");
			}

			// Return driver error indicating device is not supported.
			Err(DriverError::InitVirtioDevFail(
				VirtioError::DevNotSupported(device_id),
			))
		}
	}
}

pub(crate) enum VirtioDriver {
	#[cfg(feature = "virtio-console")]
	Console(alloc::boxed::Box<VirtioConsoleDriver>),
	#[cfg(feature = "virtio-fs")]
	Fs(alloc::boxed::Box<VirtioFsDriver>),
	#[cfg(all(
		not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
		not(feature = "rtl8139"),
		feature = "virtio-net",
	))]
	Net(alloc::boxed::Box<VirtioNetDriver>),
	#[cfg(feature = "virtio-vsock")]
	Vsock(alloc::boxed::Box<VirtioVsockDriver>),
}
