//! virtio-pci infrastructure.
//!
//! For details on the device, see [Virtio Over PCI Bus].
//! For details on the Rust definitions, see [`virtio::pci`].
//!
//! [Virtio Over PCI Bus]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-1150001

#![allow(dead_code)]

use alloc::vec::Vec;
use core::ptr::NonNull;
use core::{mem, ptr};

use memory_addresses::PhysAddr;
use pci_types::capability::PciCapability;
use virtio::pci::{
	CapCfgType, CapData, CommonCfg, CommonCfgVolatileFieldAccess, CommonCfgVolatileWideFieldAccess,
	IsrStatus as IsrStatusRaw, NotificationData,
};
use virtio::{DeviceStatus, le16, le32};
use volatile::access::ReadOnly;
use volatile::{VolatilePtr, VolatileRef};

use crate::arch::pci::PciConfigRegion;
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
use crate::drivers::virtio::VirtioIdExt;
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::pci::PciBar as VirtioPciBar;
#[cfg(feature = "virtio-vsock")]
use crate::drivers::vsock::VirtioVsockDriver;

/// Maps a given device specific pci configuration structure and
/// returns a static reference to it.
pub fn map_dev_cfg<T>(cap: &PciCap) -> Option<&'static mut T> {
	if cap.cap.cfg_type != CapCfgType::Device {
		error!("Capability of device config has wrong id. Mapping not possible...");
		return None;
	};

	if cap.bar_len() < cap.len() + cap.offset() {
		error!(
			"Device config of device {:x}, does not fit into memory specified by bar!",
			cap.dev_id(),
		);
		return None;
	}

	// Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
	if cap.len() < u64::try_from(mem::size_of::<T>()).unwrap() {
		error!(
			"Device specific config from device {:x}, does not represent actual structure specified by the standard!",
			cap.dev_id()
		);
		return None;
	}

	let virt_addr_raw = cap.bar_addr() + cap.offset();

	// Create mutable reference to the PCI structure in PCI memory
	let dev_cfg: &'static mut T =
		unsafe { &mut *(ptr::with_exposed_provenance_mut(virt_addr_raw.try_into().unwrap())) };

	Some(dev_cfg)
}

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
	dev_id: u16,
	cap: CapData,
}

impl PciCap {
	pub fn offset(&self) -> u64 {
		self.cap.offset.to_ne()
	}

	pub fn len(&self) -> u64 {
		self.cap.length.to_ne()
	}

	pub fn bar_len(&self) -> u64 {
		self.bar.length
	}

	pub fn bar_addr(&self) -> u64 {
		self.bar.mem_addr
	}

	pub fn dev_id(&self) -> u16 {
		self.dev_id
	}

	/// Returns a reference to the actual structure inside the PCI devices memory space.
	fn map_common_cfg(&self) -> Option<VolatileRef<'static, CommonCfg>> {
		if self.bar.length < self.len() + self.offset() {
			let dev_id = self.dev_id;
			let index = self.bar.index;
			error!(
				"Common config of the capability of device {dev_id:x} does not fit into memory specified by bar {index:x}!"
			);
			return None;
		}

		// `CommonCfg::queue_notify_data` and `CommonCfg::queue_reset` are optional.
		const MIN_SIZE: usize = mem::size_of::<CommonCfg>() - mem::size_of::<[le16; 2]>();
		if self.len() < u64::try_from(MIN_SIZE).unwrap() {
			error!("Common config does not represent actual structure specified by the standard!");
			return None;
		}

		let virt_addr_raw = self.bar.mem_addr + self.offset();
		let ptr = NonNull::new(ptr::with_exposed_provenance_mut::<CommonCfg>(
			virt_addr_raw.try_into().unwrap(),
		))
		.unwrap();

		// Create mutable reference to the PCI structure in PCI memory
		let com_cfg_raw = unsafe { VolatileRef::new(ptr) };

		Some(com_cfg_raw)
	}

	fn map_isr_status(&self) -> Option<VolatileRef<'static, IsrStatusRaw>> {
		if self.bar.length < self.len() + self.offset() {
			let dev_id = self.dev_id;
			let index = self.bar.index;
			error!(
				"ISR status config of device {dev_id:x}, does not fit into memory specified by bar {index:x}!"
			);
			return None;
		}

		let virt_addr_raw = self.bar.mem_addr + self.offset();
		let ptr = NonNull::new(ptr::with_exposed_provenance_mut::<IsrStatusRaw>(
			virt_addr_raw.try_into().unwrap(),
		))
		.unwrap();

		// Create mutable reference to the PCI structure in the devices memory area
		let isr_stat_raw = unsafe { VolatileRef::new(ptr) };

		Some(isr_stat_raw)
	}
}

/// Universal Caplist Collections holds all universal capability structures for
/// a given Virtio PCI device.
///
/// As Virtio's PCI devices are allowed to present multiple capability
/// structures of the same config type, the structure
/// provides a driver with all capabilities, sorted in descending priority,
/// allowing the driver to choose.
/// The structure contains a special dev_cfg_list field, a vector holding
/// [PciCap] objects, to allow the driver to map its
/// device specific configurations independently.
pub struct UniCapsColl {
	pub(crate) com_cfg: ComCfg,
	pub(crate) notif_cfg: NotifCfg,
	pub(crate) isr_cfg: IsrStatus,
	pub(crate) sh_mem_cfg_list: Vec<ShMemCfg>,
	pub(crate) dev_cfg_list: Vec<PciCap>,
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
	/// Select a queue via an index. If queue does NOT exist returns `None`, else
	/// returns `Some(VqCfgHandler)`.
	///
	/// INFO: The queue size is automatically bounded by constant `src::config:VIRTIO_MAX_QUEUE_SIZE`.
	pub fn select_vq(&mut self, index: u16) -> Option<VqCfgHandler<'_>> {
		self.com_cfg.as_mut_ptr().queue_select().write(index.into());

		if self.com_cfg.as_mut_ptr().queue_size().read().to_ne() == 0 {
			None
		} else {
			Some(VqCfgHandler {
				vq_index: index,
				raw: self.com_cfg.borrow_mut(),
			})
		}
	}

	pub fn device_config_space(&self) -> VolatilePtr<'_, CommonCfg, ReadOnly> {
		self.com_cfg.as_ptr()
	}

	/// Returns the device status field.
	pub fn dev_status(&self) -> u8 {
		self.com_cfg.as_ptr().device_status().read().bits()
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

	/// Returns the features offered by the device.
	pub fn dev_features(&mut self) -> virtio::F {
		let com_cfg = self.com_cfg.as_mut_ptr();
		let device_feature_select = com_cfg.device_feature_select();
		let device_feature = com_cfg.device_feature();

		// Indicate device to show high 32 bits in device_feature field.
		// See Virtio specification v1.1. - 4.1.4.3
		device_feature_select.write(1.into());

		// read high 32 bits of device features
		let mut device_features = u64::from(device_feature.read().to_ne()) << 32;

		// Indicate device to show low 32 bits in device_feature field.
		// See Virtio specification v1.1. - 4.1.4.3
		device_feature_select.write(0.into());

		// read low 32 bits of device features
		device_features |= u64::from(device_feature.read().to_ne());

		virtio::F::from_bits_retain(u128::from(device_features).into())
	}

	/// Write selected features into driver_select field.
	pub fn set_drv_features(&mut self, features: virtio::F) {
		let features = features.bits().to_ne() as u64;
		let com_cfg = self.com_cfg.as_mut_ptr();
		let driver_feature_select = com_cfg.driver_feature_select();
		let driver_feature = com_cfg.driver_feature();

		let high: u32 = (features >> 32) as u32;
		let low: u32 = features as u32;

		// Indicate to device that driver_features field shows low 32 bits.
		// See Virtio specification v1.1. - 4.1.4.3
		driver_feature_select.write(0.into());

		// write low 32 bits of device features
		driver_feature.write(low.into());

		// Indicate to device that driver_features field shows high 32 bits.
		// See Virtio specification v1.1. - 4.1.4.3
		driver_feature_select.write(1.into());

		// write high 32 bits of device features
		driver_feature.write(high.into());
	}
}

/// Notification Structure to handle virtqueue notification settings.
/// See Virtio specification v1.1 - 4.1.4.4
pub struct NotifCfg {
	/// Start addr, from where the notification addresses for the virtqueues are computed
	base_addr: u64,
	notify_off_multiplier: u32,
	/// defines the maximum size of the notification space, starting from base_addr.
	length: u64,
}

impl NotifCfg {
	fn new(cap: &PciCap) -> Option<Self> {
		if cap.bar.length < cap.len() + cap.offset() {
			let dev_id = cap.dev_id;
			let index = cap.bar.index;
			error!(
				"Notification config of device {dev_id:x}, does not fit into memory specified by bar {index:x}!"
			);
			return None;
		}

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
			length: cap.len(),
		})
	}

	pub fn notification_location(&self, vq_cfg_handler: &mut VqCfgHandler<'_>) -> *mut le32 {
		let addend = u32::from(vq_cfg_handler.notif_off()) * self.notify_off_multiplier;
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

	pub fn is_queue_interrupt(&self) -> IsrStatusRaw {
		self.isr_stat.as_ptr().read()
	}

	pub fn acknowledge(&mut self) {
		// nothing to do
	}
}

/// Shared memory configuration structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.7
///
/// Each shared memory region is defined via a single shared
/// memory structure. Each region is identified by an id indicated
/// via the capability.id field of PciCapRaw.
///
/// The shared memory region is defined via a PciCap64 structure.
/// See Virtio specification v.1.1 - 4.1.4 for structure.
///
// Only used for capabilities that require offsets or lengths
// larger than 4GB.
// #[repr(C)]
// struct PciCap64 {
//    pci_cap: PciCap,
//    offset_hi: u32,
//    length_hi: u32
pub struct ShMemCfg {
	mem_addr: u64,
	length: u64,
	sh_mem: ShMem,
	/// Shared memory regions are identified via an ID
	/// See Virtio specification v1.1. - 4.1.4.7
	id: u8,
}

impl ShMemCfg {
	fn new(cap: &PciCap) -> Option<Self> {
		if cap.bar.length < cap.len() + cap.offset() {
			error!(
				"Shared memory config of with id {} of device {:x}, does not fit into memory specified by bar {:x}!",
				cap.cap.id, cap.dev_id, cap.bar.index
			);
			return None;
		}

		let offset = cap.cap.offset.to_ne();
		let length = cap.cap.length.to_ne();

		let virt_addr_raw = cap.bar.mem_addr + offset;
		let raw_ptr = ptr::with_exposed_provenance_mut::<u8>(virt_addr_raw.try_into().unwrap());

		// Zero initialize shared memory area
		unsafe {
			for i in 0..usize::try_from(length).unwrap() {
				*(raw_ptr.add(i)) = 0;
			}
		};

		// Currently in place in order to ensure a safe cast below
		// "len: cap.bar.length as usize"
		// In order to remove this assert a safe conversion from
		// kernel PciBar struct into usize must be made
		assert!(mem::size_of::<usize>() == 8);

		Some(ShMemCfg {
			mem_addr: virt_addr_raw,
			length: cap.len(),
			sh_mem: ShMem {
				ptr: raw_ptr,
				len: cap.bar.length as usize,
			},
			id: cap.cap.id,
		})
	}
}

/// Defines a shared memory locate at location ptr with a length of len.
/// The shared memories Drop implementation does not dealloc the memory
/// behind the pointer but sets it to zero, to prevent leakage of data.
struct ShMem {
	ptr: *mut u8,
	len: usize,
}

impl core::ops::Deref for ShMem {
	type Target = [u8];

	fn deref(&self) -> &[u8] {
		unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
	}
}

impl core::ops::DerefMut for ShMem {
	fn deref_mut(&mut self) -> &mut [u8] {
		unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
	}
}

// Upon drop the shared memory region is "deleted" with zeros.
impl Drop for ShMem {
	fn drop(&mut self) {
		for i in 0..self.len {
			unsafe {
				*(self.ptr.add(i)) = 0;
			}
		}
	}
}

/// PciBar stores the virtual memory address and associated length of memory space
/// a PCI device's physical memory indicated by the device's BAR has been mapped to.
//
// Currently all fields are public as the struct is instantiated in the drivers::virtio::env module
#[derive(Copy, Clone, Debug)]
pub struct PciBar {
	index: u8,
	mem_addr: u64,
	length: u64,
}

impl PciBar {
	pub fn new(index: u8, mem_addr: u64, length: u64) -> Self {
		PciBar {
			index,
			mem_addr,
			length,
		}
	}
}

/// Reads all PCI capabilities, starting at the capabilities list pointer from the
/// PCI device.
///
/// Returns ONLY Virtio specific capabilities, which allow to locate the actual capability
/// structures inside the memory areas, indicated by the BaseAddressRegisters (BAR's).
fn read_caps(device: &PciDevice<PciConfigRegion>) -> Result<Vec<PciCap>, PciError> {
	let device_id = device.device_id();

	let capabilities = device
		.capabilities()
		.unwrap()
		.filter_map(|capability| match capability {
			PciCapability::Vendor(capability) => Some(capability),
			_ => None,
		})
		.map(|addr| CapData::read(addr, device.access()).unwrap())
		.filter(|cap| cap.cfg_type != CapCfgType::Pci)
		.flat_map(|cap| {
			let slot = cap.bar;
			device
				.memory_map_bar(slot, true)
				.map(|(addr, size)| PciCap {
					bar: VirtioPciBar::new(slot, addr.as_u64(), size.try_into().unwrap()),
					dev_id: device_id,
					cap,
				})
		})
		.collect::<Vec<_>>();

	if capabilities.is_empty() {
		error!("No virtio capability found for device {device_id:x}");
		Err(PciError::NoVirtioCaps(device_id))
	} else {
		Ok(capabilities)
	}
}

pub(crate) fn map_caps(device: &PciDevice<PciConfigRegion>) -> Result<UniCapsColl, VirtioError> {
	let device_id = device.device_id();

	// In case caplist pointer is not used, abort as it is essential
	if !device.status().has_capability_list() {
		error!("Found virtio device without capability list. Aborting!");
		return Err(VirtioError::FromPci(PciError::NoCapPtr(device_id)));
	}

	// Get list of PciCaps pointing to capabilities
	let cap_list = match read_caps(device) {
		Ok(list) => list,
		Err(pci_error) => return Err(VirtioError::FromPci(pci_error)),
	};

	let mut com_cfg = None;
	let mut notif_cfg = None;
	let mut isr_cfg = None;
	let mut sh_mem_cfg_list = Vec::new();
	let mut dev_cfg_list = Vec::new();
	// Map Caps in virtual memory
	for pci_cap in cap_list {
		match pci_cap.cap.cfg_type {
			CapCfgType::Common => {
				if com_cfg.is_none() {
					match pci_cap.map_common_cfg() {
						Some(cap) => com_cfg = Some(ComCfg::new(cap)),
						None => error!(
							"Common config capability of device {device_id:x} could not be mapped!"
						),
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
				if isr_cfg.is_none() {
					match pci_cap.map_isr_status() {
						Some(isr_stat) => isr_cfg = Some(IsrStatus::new(isr_stat)),
						None => error!(
							"ISR status config capability of device {device_id:x} could not be used!"
						),
					}
				}
			}
			CapCfgType::SharedMemory => match ShMemCfg::new(&pci_cap) {
				Some(sh_mem) => sh_mem_cfg_list.push(sh_mem),
				None => {
					let cap_id = pci_cap.cap.id;
					error!(
						"Shared Memory config capability with id {cap_id} of device {device_id:x} could not be used!"
					);
				}
			},
			CapCfgType::Device => dev_cfg_list.push(pci_cap),

			// PCI's configuration space is allowed to hold other structures, which are not virtio specific and are therefore ignored
			// in the following
			_ => continue,
		}
	}

	Ok(UniCapsColl {
		com_cfg: com_cfg.ok_or(VirtioError::NoComCfg(device_id))?,
		notif_cfg: notif_cfg.ok_or(VirtioError::NoNotifCfg(device_id))?,
		isr_cfg: isr_cfg.ok_or(VirtioError::NoIsrCfg(device_id))?,
		sh_mem_cfg_list,
		dev_cfg_list,
	})
}

/// Checks existing drivers for support of given device. Upon match, provides
/// driver with a [`PciDevice<PciConfigRegion>`] reference, allowing access to the capabilities
/// list of the given device through [map_caps].
pub(crate) fn init_device(
	device: &PciDevice<PciConfigRegion>,
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
		#[cfg(all(
			not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
			not(feature = "rtl8139"),
			feature = "virtio-net",
		))]
		virtio::Id::Net => match VirtioNetDriver::init(device) {
			Ok(virt_net_drv) => {
				info!("Virtio network driver initialized.");

				let irq = device.get_irq().unwrap();
				crate::arch::interrupts::add_irq_name(irq, "virtio");
				info!("Virtio interrupt handler at line {irq}");

				Ok(VirtioDriver::Network(alloc::boxed::Box::new(virt_net_drv)))
			}
			Err(virtio_error) => {
				error!(
					"Virtio networkd driver could not be initialized with device: {device_id:x}"
				);
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		#[cfg(feature = "virtio-console")]
		virtio::Id::Console => match VirtioConsoleDriver::init(device) {
			Ok(virt_console_drv) => {
				info!("Virtio console driver initialized.");

				let irq = device.get_irq().unwrap();
				crate::arch::interrupts::add_irq_name(irq, "virtio");
				info!("Virtio interrupt handler at line {irq}");

				Ok(VirtioDriver::Console(alloc::boxed::Box::new(
					virt_console_drv,
				)))
			}
			Err(virtio_error) => {
				error!("Virtio console driver could not be initialized with device: {device_id:x}");
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		#[cfg(feature = "virtio-vsock")]
		virtio::Id::Vsock => match VirtioVsockDriver::init(device) {
			Ok(virt_sock_drv) => {
				info!("Virtio sock driver initialized.");

				let irq = device.get_irq().unwrap();
				crate::arch::interrupts::add_irq_name(irq, "virtio");
				info!("Virtio interrupt handler at line {irq}");

				Ok(VirtioDriver::Vsock(alloc::boxed::Box::new(virt_sock_drv)))
			}
			Err(virtio_error) => {
				error!("Virtio sock driver could not be initialized with device: {device_id:x}");
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		#[cfg(feature = "virtio-fs")]
		virtio::Id::Fs => {
			// TODO: check subclass
			// TODO: proper error handling on driver creation fail
			match VirtioFsDriver::init(device) {
				Ok(virt_fs_drv) => {
					info!("Virtio filesystem driver initialized.");
					Ok(VirtioDriver::FileSystem(alloc::boxed::Box::new(
						virt_fs_drv,
					)))
				}
				Err(virtio_error) => {
					error!(
						"Virtio filesystem driver could not be initialized with device: {device_id:x}"
					);
					Err(DriverError::InitVirtioDevFail(virtio_error))
				}
			}
		}
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
	#[cfg(all(
		not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
		not(feature = "rtl8139"),
		feature = "virtio-net",
	))]
	Network(alloc::boxed::Box<VirtioNetDriver>),
	#[cfg(feature = "virtio-console")]
	Console(alloc::boxed::Box<VirtioConsoleDriver>),
	#[cfg(feature = "virtio-vsock")]
	Vsock(alloc::boxed::Box<VirtioVsockDriver>),
	#[cfg(feature = "virtio-fs")]
	FileSystem(alloc::boxed::Box<VirtioFsDriver>),
}
