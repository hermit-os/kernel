// Copyright (c) 2020 Thomas Lambertz, RWTH Aachen University
//               2020 Frederik Schulz, RWTH Aachen Univerity
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing all virtio specific pci functionality
//!
//! The module contains ...
#![allow(dead_code)]

use crate::arch::kernel::pci as kernel_pci;
use crate::arch::kernel::pci::error::PciError;
use crate::arch::kernel::pci::{PciAdapter, PciDriver};
use crate::arch::mm::PhysAddr;
use crate::synch::spinlock::SpinlockIrqSave;
use alloc::vec::Vec;
use core::convert::TryInto;
use core::mem;
use core::result::Result;

use crate::drivers::error::DriverError;
use crate::drivers::net::virtio_net::VirtioNetDriver;
use crate::drivers::virtio::device;
use crate::drivers::virtio::env;
use crate::drivers::virtio::env::memory::{MemLen, MemOff, VirtMemAddr};
use crate::drivers::virtio::error::VirtioError;

use crate::arch::x86_64::kernel::irq::*;
use crate::drivers::net::network_irqhandler;
use crate::drivers::virtio::depr::virtio_fs;

/// Virtio device ID's
/// See Virtio specification v1.1. - 5
///                      and v1.1. - 4.1.2.1
///
// WARN: Upon changes in the set of the enum variants
// one MUST adjust the associated From<u16>
// implementation, in order catch all cases correctly,
// as this function uses the catch-all "_" case!
#[allow(dead_code, non_camel_case_types)]
#[repr(u16)]
pub enum DevId {
	INVALID = 0x0,
	VIRTIO_TRANS_DEV_ID_NET = 0x1000,
	VIRTIO_TRANS_DEV_ID_BLK = 0x1001,
	VIRTIO_TRANS_DEV_ID_MEM_BALL = 0x1002,
	VIRTIO_TRANS_DEV_ID_CONS = 0x1003,
	VIRTIO_TRANS_DEV_ID_SCSI = 0x1004,
	VIRTIO_TRANS_DEV_ID_ENTROPY = 0x1005,
	VIRTIO_TRANS_DEV_ID_9P = 0x1009,
	VIRTIO_DEV_ID_NET = 0x1041,
	VIRTIO_DEV_ID_FS = 0x105A,
}

impl From<DevId> for u16 {
	fn from(val: DevId) -> u16 {
		match val {
			DevId::VIRTIO_TRANS_DEV_ID_NET => 0x1000,
			DevId::VIRTIO_TRANS_DEV_ID_BLK => 0x1001,
			DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL => 0x1002,
			DevId::VIRTIO_TRANS_DEV_ID_CONS => 0x1003,
			DevId::VIRTIO_TRANS_DEV_ID_SCSI => 0x1004,
			DevId::VIRTIO_TRANS_DEV_ID_ENTROPY => 0x1005,
			DevId::VIRTIO_TRANS_DEV_ID_9P => 0x1009,
			DevId::VIRTIO_DEV_ID_NET => 0x1041,
			DevId::VIRTIO_DEV_ID_FS => 0x105A,
			DevId::INVALID => 0x0,
		}
	}
}

impl From<u16> for DevId {
	fn from(val: u16) -> Self {
		match val {
			0x1000 => DevId::VIRTIO_TRANS_DEV_ID_NET,
			0x1001 => DevId::VIRTIO_TRANS_DEV_ID_BLK,
			0x1002 => DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL,
			0x1003 => DevId::VIRTIO_TRANS_DEV_ID_CONS,
			0x1004 => DevId::VIRTIO_TRANS_DEV_ID_SCSI,
			0x1005 => DevId::VIRTIO_TRANS_DEV_ID_ENTROPY,
			0x1009 => DevId::VIRTIO_TRANS_DEV_ID_9P,
			0x1041 => DevId::VIRTIO_DEV_ID_NET,
			0x105A => DevId::VIRTIO_DEV_ID_FS,
			_ => DevId::INVALID,
		}
	}
}

/// Virtio's cfg_type constants; indicating type of structure in capabilities list
/// See Virtio specification v1.1 - 4.1.4
//
// WARN: Upon changes in the set of the enum variants
// one MUST adjust the associated From<u8>
// implementation, in order catch all cases correctly,
// as this function uses the catch-all "_" case!
#[allow(dead_code, non_camel_case_types)]
#[derive(Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum CfgType {
	INVALID = 0,
	VIRTIO_PCI_CAP_COMMON_CFG = 1,
	VIRTIO_PCI_CAP_NOTIFY_CFG = 2,
	VIRTIO_PCI_CAP_ISR_CFG = 3,
	VIRTIO_PCI_CAP_DEVICE_CFG = 4,
	VIRTIO_PCI_CAP_PCI_CFG = 5,
	VIRTIO_PCI_CAP_SHARED_MEMORY_CFG = 8,
}

impl From<CfgType> for u8 {
	fn from(val: CfgType) -> u8 {
		match val {
			CfgType::INVALID => 0,
			CfgType::VIRTIO_PCI_CAP_COMMON_CFG => 1,
			CfgType::VIRTIO_PCI_CAP_NOTIFY_CFG => 2,
			CfgType::VIRTIO_PCI_CAP_ISR_CFG => 3,
			CfgType::VIRTIO_PCI_CAP_DEVICE_CFG => 4,
			CfgType::VIRTIO_PCI_CAP_PCI_CFG => 5,
			CfgType::VIRTIO_PCI_CAP_SHARED_MEMORY_CFG => 8,
		}
	}
}

impl From<u8> for CfgType {
	fn from(val: u8) -> Self {
		match val {
			1 => CfgType::VIRTIO_PCI_CAP_COMMON_CFG,
			2 => CfgType::VIRTIO_PCI_CAP_NOTIFY_CFG,
			3 => CfgType::VIRTIO_PCI_CAP_ISR_CFG,
			4 => CfgType::VIRTIO_PCI_CAP_DEVICE_CFG,
			5 => CfgType::VIRTIO_PCI_CAP_PCI_CFG,
			8 => CfgType::VIRTIO_PCI_CAP_SHARED_MEMORY_CFG,
			_ => CfgType::INVALID,
		}
	}
}

/// Public structure to allow drivers to read the configuration space
/// savely
#[derive(Clone)]
pub struct Origin {
	cfg_ptr: u32, // Register to be read to reach configuration structure of type cfg_type
	dev: u8,      // PCI device this configuration comes from
	bus: u8,      // Bus of the PCI device
	dev_id: u16,
	cap_struct: PciCapRaw,
}

/// Maps a given device specific pci configuration structure and
/// returns a static reference to it.
pub fn map_dev_cfg<T>(cap: &PciCap) -> Option<&'static mut T> {
	if cap.cfg_type != CfgType::VIRTIO_PCI_CAP_DEVICE_CFG {
		error!("Capability of device config has wrong id. Mapping not possible...");
		return None;
	};

	if cap.bar_len() < u64::from(cap.len() + cap.offset()) {
		error!(
			"Device config of device {:x}, does not fit into memeory specified by bar!",
			cap.dev_id(),
		);
		return None;
	}

	// Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
	if cap.len() < MemLen::from(mem::size_of::<T>()) {
		error!("Device specific config from device {:x}, does not represent actual structure specified by the standard!", cap.dev_id());
		return None;
	}

	let virt_addr_raw: VirtMemAddr = cap.bar_addr() + cap.offset();

	// Create mutable reference to the PCI structure in PCI memory
	let dev_cfg: &'static mut T = unsafe { &mut *(usize::from(virt_addr_raw) as *mut T) };

	Some(dev_cfg)
}

/// Virtio's PCI capabilites structure.
/// See Virtio specification v.1.1 - 4.1.4
///
/// Indicating: Where the capability field is mapped in memory and
/// Which id (sometimes also indicates priority for multiple
/// capabilites of same type) it holds.
///
/// This structure does NOT represent the structure in the standard,
/// as it is not directly mapped into address space from PCI device
/// configuration space.
/// Therefore the struct only contains necessary information to map
/// corresponding [CfgType](enums.CfgType.html) into address space.
#[derive(Clone)]
pub struct PciCap {
	cfg_type: CfgType,
	bar: PciBar,
	id: u8,
	offset: MemOff,
	length: MemLen,
	// Following field can be used to retrieve original structure
	// from the config space. Needed by some structures and f
	// device specific configs.
	origin: Origin,
}

impl PciCap {
	pub fn offset(&self) -> MemOff {
		self.offset
	}

	pub fn len(&self) -> MemLen {
		self.length
	}

	pub fn bar_len(&self) -> u64 {
		self.bar.length
	}

	pub fn bar_addr(&self) -> VirtMemAddr {
		self.bar.mem_addr
	}

	pub fn dev_id(&self) -> u16 {
		self.origin.dev_id
	}
}

/// Virtio's PCI capabilites structure.
/// See Virtio specification v.1.1 - 4.1.4
///
/// WARN: endianness of this structure should be seen as little endian.
/// As this structure is not meant to be used outside of this module and for
/// ease of conversion from reading data into struct from PCI configuration
/// space, no conversion is made for struct fields.
#[derive(Clone)]
#[repr(C)]
struct PciCapRaw {
	cap_vndr: u8,
	cap_next: u8,
	cap_len: u8,
	cfg_type: u8,
	bar_index: u8,
	id: u8,
	padding: [u8; 2],
	offset: u32,
	length: u32,
}

// This only shows compiler, that structs are identical
// with themselves.
impl Eq for PciCapRaw {}

// In order to compare two PciCapRaw structs PartialEq is needed
impl PartialEq for PciCapRaw {
	fn eq(&self, other: &Self) -> bool {
		if self.cap_vndr == other.cap_vndr
			&& self.cap_next == other.cap_next
			&& self.cap_len == other.cap_len
			&& self.cfg_type == other.cfg_type
			&& self.bar_index == other.bar_index
			&& self.id == other.id
			&& self.offset == other.offset
			&& self.length == other.length
		{
			true
		} else {
			false
		}
	}
}

/// Universal Caplist Collections holds all universal capability structures for
/// a given Virtio PCI device.
///
/// As Virtio's PCI devices are allowed to present multiple capability
/// structures of the same [CfgType](enums.cfgtype.html), the structure
/// provides a driver with all capabilites, sorted in descending priority,
/// allowing the driver to choose.
/// The structure contains a special dev_cfg_list field, a vector holding
/// [PciCap](structs.pcicap.html) objects, to allow the driver to map its
/// device specific configurations independently.
pub struct UniCapsColl {
	com_cfg_list: Vec<ComCfg>,
	notif_cfg_list: Vec<NotifCfg>,
	isr_stat_list: Vec<IsrStatus>,
	pci_cfg_acc_list: Vec<PciCfgAlt>,
	sh_mem_cfg_list: Vec<ShMemCfg>,
	dev_cfg_list: Vec<PciCap>,
}

impl UniCapsColl {
	/// Returns an Caps with empty lists.
	fn new() -> Self {
		UniCapsColl {
			com_cfg_list: Vec::new(),
			notif_cfg_list: Vec::new(),
			isr_stat_list: Vec::new(),
			pci_cfg_acc_list: Vec::new(),
			sh_mem_cfg_list: Vec::new(),
			dev_cfg_list: Vec::new(),
		}
	}

	fn add_cfg_common(&mut self, com: ComCfg) {
		self.com_cfg_list.push(com);
		// Resort array
		//
		// This should not be to expensive, as "rational" devices will hold an
		// acceptibal amount of configuration structures.
		self.com_cfg_list.sort_by(|a, b| b.rank.cmp(&a.rank));
	}

	fn add_cfg_notif(&mut self, notif: NotifCfg) {
		self.notif_cfg_list.push(notif);
		// Resort array
		//
		// This should not be to expensive, as "rational" devices will hold an
		// acceptable amount of configuration structures.
		self.notif_cfg_list.sort_by(|a, b| b.rank.cmp(&a.rank));
	}

	fn add_cfg_isr(&mut self, isr_stat: IsrStatus) {
		self.isr_stat_list.push(isr_stat);
		// Resort array
		//
		// This should not be to expensive, as "rational" devices will hold an
		// acceptable amount of configuration structures.
		self.isr_stat_list.sort_by(|a, b| b.rank.cmp(&a.rank));
	}

	fn add_cfg_alt(&mut self, pci_alt: PciCfgAlt) {
		self.pci_cfg_acc_list.push(pci_alt);
		// Resort array
		//
		// This should not be to expensive, as "rational" devices will hold an
		// acceptable amount of configuration structures.
		self.pci_cfg_acc_list
			.sort_by(|a, b| b.pci_cap.id.cmp(&a.pci_cap.id));
	}

	fn add_cfg_sh_mem(&mut self, sh_mem: ShMemCfg) {
		self.sh_mem_cfg_list.push(sh_mem);
		// Resort array
		//
		// This should not be to expensive, as "rational" devices will hold an
		// acceptable amount of configuration structures.
		self.sh_mem_cfg_list.sort_by(|a, b| b.id.cmp(&a.id));
	}

	fn add_cfg_dev(&mut self, pci_cap: PciCap) {
		self.dev_cfg_list.push(pci_cap);
		// Resort array
		//
		// This should not be to expensive, as "rational" devices will hold an
		// acceptable amount of configuration structures.
		self.dev_cfg_list.sort_by(|a, b| b.id.cmp(&a.id));
	}
}

// Public interface of UniCapsCollection
impl UniCapsColl {
	/// Returns the highest prioritized PciCap that indiactes a
	/// Virito device configuration.
	///
	/// INFO: This function removes the capability and returns ownership.
	pub fn get_dev_cfg(&mut self) -> Option<PciCap> {
		self.dev_cfg_list.pop()
	}

	/// Returns the highest prioritized common configuration structure.
	///
	/// INFO: This function removes the capability and returns ownership.
	pub fn get_com_cfg(&mut self) -> Option<ComCfg> {
		self.com_cfg_list.pop()
	}

	/// Returns the highest prioritized ISR status configuraiton structure.
	///
	/// INFO: This function removes the Capability and returns ownership.
	pub fn get_isr_cfg(&mut self) -> Option<IsrStatus> {
		self.isr_stat_list.pop()
	}

	/// Returns the highest prioritized notification structure.
	///
	/// INFO: This function removes the Capability and returns ownership.
	pub fn get_notif_cfg(&mut self) -> Option<NotifCfg> {
		self.notif_cfg_list.pop()
	}
}

/// Wraps a [ComCfgRaw](structs.comcfgraw.html) in order to preserve
/// the original structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via
/// the structure.
pub struct ComCfg {
	/// References the raw structure in PCI memory space. Is static as
	/// long as the device is present, which is mandatory in order to let this code work.
	com_cfg: &'static mut ComCfgRaw,
	/// Preferences of the device for this config. From 1 (highest) to 2^7-1 (lowest)
	rank: u8,
}

// Private interface of ComCfg
impl ComCfg {
	fn new(raw: &'static mut ComCfgRaw, rank: u8) -> Self {
		ComCfg { com_cfg: raw, rank }
	}
}

pub struct VqCfgHandler<'a> {
	vq_index: u16,
	raw: &'a mut ComCfgRaw,
}

impl<'a> VqCfgHandler<'a> {
	/// Sets the size of a given virtqueue. In case the provided size exceeds the maximum allowed
	/// size, the size is set to this maximum instead. Else size is set to the provided value.
	///
	/// Returns the set size in form of a `u16`.
	pub fn set_vq_size(&mut self, size: u16) -> u16 {
		self.raw.queue_select = self.vq_index;

		if self.raw.queue_size < size {
			self.raw.queue_size
		} else {
			self.raw.queue_size = size;
			self.raw.queue_size
		}
	}

	pub fn set_ring_addr(&mut self, addr: PhysAddr) {
		self.raw.queue_select = self.vq_index;
		self.raw.queue_desc = addr.as_u64();
	}

	pub fn set_drv_ctrl_addr(&mut self, addr: PhysAddr) {
		self.raw.queue_select = self.vq_index;
		self.raw.queue_driver = addr.as_u64();
	}

	pub fn set_dev_ctrl_addr(&mut self, addr: PhysAddr) {
		self.raw.queue_select = self.vq_index;
		self.raw.queue_device = addr.as_u64();
	}

	pub fn notif_off(&mut self) -> u16 {
		self.raw.queue_select = self.vq_index;
		self.raw.queue_notify_off
	}

	pub fn enable_queue(&mut self) {
		self.raw.queue_select = self.vq_index;
		self.raw.queue_enable = 1;
	}
}

// Public Interface of ComCfg
impl ComCfg {
	/// Select a queue via an index. If queue does NOT exist returns `None`, else
	/// returns `Some(VqCfgHandler)`.
	///
	/// INFO: The queue size is automatically bounded by constant `src::config:VIRTIO_MAX_QUEUE_SIZE`.
	pub fn select_vq(&mut self, index: u16) -> Option<VqCfgHandler> {
		self.com_cfg.queue_select = index;

		if self.com_cfg.queue_size == 0 {
			None
		} else {
			Some(VqCfgHandler {
				vq_index: index,
				raw: self.com_cfg,
			})
		}
	}

	/// Returns the device status field.
	pub fn dev_status(&self) -> u8 {
		self.com_cfg.device_status
	}

	/// Resets the device status field to zero.
	pub fn reset_dev(&mut self) {
		self.com_cfg.device_status = 0;
	}

	/// Sets the device status field to FAILED.
	/// A driver MUST NOT initialize and use the device any further after this.
	/// A driver MAY use the device again after a proper reset of the device.
	pub fn set_failed(&mut self) {
		self.com_cfg.device_status = u8::from(device::Status::FAILED);
	}

	/// Sets the ACKNOWLEDGE bit in the device status field. This indicates, the
	/// OS has notived the device
	pub fn ack_dev(&mut self) {
		self.com_cfg.device_status |= u8::from(device::Status::ACKNOWLEDGE);
	}

	/// Sets the DRIVER bit in the device status field. This indicates, the OS
	/// know how to run this device.
	pub fn set_drv(&mut self) {
		self.com_cfg.device_status |= u8::from(device::Status::DRIVER);
	}

	/// Sets the FEATURES_OK bit in the device status field.
	///
	/// Drivers MUST NOT accept new features after this step.
	pub fn features_ok(&mut self) {
		self.com_cfg.device_status |= u8::from(device::Status::FEATURES_OK);
	}

	/// In order to correctly check feature negotiaten, this function
	/// MUST be called after [self.features_ok()](ComCfg::features_ok()) in order to check
	/// if features have been accepted by the device after negotiation.
	///
	/// Re-reads device status to ensure the FEATURES_OK bit is still set:
	/// otherwise, the device does not support our subset of features and the device is unusable.
	pub fn check_features(&self) -> bool {
		self.com_cfg.device_status & u8::from(device::Status::FEATURES_OK)
			== u8::from(device::Status::FEATURES_OK)
	}

	/// Sets the DRIVER_OK bit in the device status field.
	///
	/// After this call, the device is "live"!
	pub fn drv_ok(&mut self) {
		self.com_cfg.device_status |= u8::from(device::Status::DRIVER_OK)
	}

	/// Returns the features offered by the device. Coded in a 64bit value.
	pub fn dev_features(&mut self) -> u64 {
		// Indicate device to show high 32 bits in device_feature field.
		// See Virtio specification v1.1. - 4.1.4.3
		self.com_cfg.device_feature_select = 1;

		// read high 32 bits of device features
		let mut dev_feat = u64::from(self.com_cfg.device_feature) << 32;

		// Indicate device to show low 32 bits in device_feature field.
		// See Virtio specification v1.1. - 4.1.4.3
		self.com_cfg.device_feature_select = 0;

		// read low 32 bits of device features
		dev_feat |= u64::from(self.com_cfg.device_feature);

		dev_feat
	}

	/// Write selected features into driver_select field.
	pub fn set_drv_features(&mut self, feats: u64) {
		let high: u32 = (feats >> 32) as u32;
		let low: u32 = feats as u32;

		// Indicate to device that driver_features field shows low 32 bits.
		// See Virtio specification v1.1. - 4.1.4.3
		self.com_cfg.driver_feature_select = 0;

		// write low 32 bits of device features
		self.com_cfg.driver_feature = low;

		// Indicate to device that driver_features field shows high 32 bits.
		// See Virtio specification v1.1. - 4.1.4.3
		self.com_cfg.driver_feature_select = 1;

		// write high 32 bits of device features
		self.com_cfg.driver_feature = high;
	}
}

/// Common configuration structure of Virtio PCI devices.
/// See Virtio specification v1.1 - 4.1.43
///
/// Fields read-write-rules in source code refer to driver rights.
#[repr(C)]
struct ComCfgRaw {
	// About whole device
	device_feature_select: u32, // read-write
	device_feature: u32,        // read-only for driver
	driver_feature_select: u32, // read-write
	driver_feature: u32,        // read-write
	config_msix_vector: u16,    // read-write
	num_queues: u16,            // read-only for driver
	device_status: u8,          // read-write
	config_generation: u8,      // read-only for driver

	// About a specific virtqueue
	queue_select: u16,      // read-write
	queue_size: u16,        // read-write
	queue_msix_vector: u16, // read-write
	queue_enable: u16,      // read-write
	queue_notify_off: u16,  // read-only for driver. Offset of the notification area.
	queue_desc: u64,        // read-write
	queue_driver: u64,      // read-write
	queue_device: u64,      // read-write
}

// Common configuration raw does NOT provide a PUBLIC
// interface.
impl ComCfgRaw {
	/// Returns a boxed [ComCfgRaw](ComCfgRaw) structure. The box points to the actual structure inside the
	/// PCI devices memory space.
	fn map(cap: &PciCap) -> Option<&'static mut ComCfgRaw> {
		if cap.bar.length < u64::from(cap.length + cap.offset) {
			error!("Common config of with id {} of device {:x}, does not fit into memeory specified by bar {:x}!", 
                cap.id,
                cap.origin.dev_id,
                 cap.bar.index
            );
			return None;
		}

		// Using "as u32" is safe here as ComCfgRaw has a defined size smaller 2^31-1
		// Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
		if cap.length < MemLen::from(mem::size_of::<ComCfgRaw>() * 8) {
			error!("Common config of with id {}, does not represent actual structure specified by the standard!", cap.id);
			return None;
		}

		let virt_addr_raw = cap.bar.mem_addr + cap.offset;

		// Create mutable reference to the PCI structure in PCI memory
		let com_cfg_raw: &mut ComCfgRaw =
			unsafe { &mut *(usize::from(virt_addr_raw) as *mut ComCfgRaw) };

		Some(com_cfg_raw)
	}
}

/// Notification Structure to handle virtqueue notification settings.
/// See Virtio specification v1.1 - 4.1.4.4
//
pub struct NotifCfg {
	/// Start addr, from where the notification addresses for the virtqueues are computed
	base_addr: VirtMemAddr,
	notify_off_multiplier: u32,
	/// Preferences of the device for this config. From 1 (highest) to 2^7-1 (lowest)
	rank: u8,
	/// defines the maximum size of the notification space, starting from base_addr.
	length: MemLen,
}

impl NotifCfg {
	fn new(cap: &PciCap) -> Option<Self> {
		if cap.bar.length < u64::from(u32::from(cap.length + cap.offset)) {
			error!("Notification config with id {} of device {:x}, does not fit into memeory specified by bar {:x}!", 
                cap.id,
                cap.origin.dev_id,
                cap.bar.index
            );
			return None;
		}

		// Assumes the cap_len is a multiple of 8
		// This read MIGHT be slow, as it does NOT ensure 32 bit alignment.
		let notify_off_multiplier = env::pci::read_cfg_no_adapter(
			cap.origin.bus,
			cap.origin.dev,
			cap.origin.cfg_ptr + u32::from(cap.origin.cap_struct.cap_len),
		);

		// define base memory address from which the actuall Queue Notify address can be derived via
		// base_addr + queue_notify_off * notify_off_multiplier.
		//
		// Where queue_notify_off is taken from the respective common configuration struct.
		// See Virtio specification v1.1. - 4.1.4.4
		//
		// Base address here already includes offset!
		let base_addr = VirtMemAddr::from(cap.bar.mem_addr + cap.offset);

		Some(NotifCfg {
			base_addr: base_addr,
			notify_off_multiplier,
			rank: cap.id,
			length: cap.length,
		})
	}

	/// Returns base address of notification area as an usize
	pub fn base(&self) -> usize {
		usize::from(self.base_addr)
	}

	/// Returns the multiplier, needed in order to calculate the
	/// notification address for a specific queue.
	pub fn multiplier(&self) -> u32 {
		self.notify_off_multiplier
	}
}

/// Control structure, allowing to notify a device via PCI bus.
/// Typcially hold by a virtqueue.
pub struct NotifCtrl {
	/// Indicates if VIRTIO_F_NOTIFICATION_DATA has been negotiated
	f_notif_data: bool,
	/// Where to write notification
	notif_addr: *mut usize,
}

impl NotifCtrl {
	/// Retunrs a new controller. By default MSI-X capabilities and VIRTIO_F_NOTIFICATION_DATA
	/// are disabled.
	pub fn new(notif_addr: *mut usize) -> Self {
		NotifCtrl {
			f_notif_data: false,
			notif_addr,
		}
	}

	/// Enables VIRTIO_F_NOTIFICATION_DATA. This changes which data is provided to the device. ONLY a good idea if Feature has been negotiated.
	pub fn enable_notif_data(&mut self) {
		self.f_notif_data = true;
	}

	pub fn notify_dev(&self, notif_data: &[u8]) {
		// See Virtio specification v.1.1. - 4.1.5.2
		// Depending in the feature negotiation, we write eitehr only the
		// virtqueue index or the index and the next position inside the queue.
		if self.f_notif_data {
			unsafe {
				let notif_area = core::slice::from_raw_parts_mut(self.notif_addr as *mut u8, 4);
				let mut notif_data = notif_data.into_iter();

				for byte in notif_area {
					*byte = *notif_data.next().unwrap();
				}
			}
		} else {
			unsafe {
				let notif_area = core::slice::from_raw_parts_mut(self.notif_addr as *mut u8, 2);
				let mut notif_data = notif_data.into_iter();

				for byte in notif_area {
					*byte = *notif_data.next().unwrap();
				}
			}
		}
	}
}

/// Wraps a [IsrStatusRaw](structs.isrstatusraw.html) in order to preserve
/// the original structure and allow interaction with the device via
/// the structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via
/// the structure.
pub struct IsrStatus {
	/// References the raw structure in PCI memory space. Is static as
	/// long as the device is present, which is mandatory in order to let this code work.
	isr_stat: &'static mut IsrStatusRaw,
	/// Preferences of the device for this config. From 1 (highest) to 2^7-1 (lowest)
	rank: u8,
}

impl IsrStatus {
	fn new(raw: &'static mut IsrStatusRaw, rank: u8) -> Self {
		IsrStatus {
			isr_stat: raw,
			rank,
		}
	}

	pub fn is_interrupt(&self) -> bool {
		self.isr_stat.flags & 1 << 0 == 1
	}

	pub fn is_cfg_change(&self) -> bool {
		self.isr_stat.flags & 1 << 1 == 1 << 1
	}
}

/// ISR status structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.5
///
/// Contains a single byte, containing the interrupt numnbers used
/// for handling interrupts.
/// The 8-bit field is read as an bitmap and allows to distinguish between
/// interrupts triggered by changes in the configuration and interrupts
/// triggered by events of a virtqueue.
///
/// Bitmap layout (from least to most significant bit in the byte):
///
/// 0 : Queue interrupt
///
/// 1 : Device configuration interrupt
///
/// 2 - 31 : Reserved
#[repr(C)]
struct IsrStatusRaw {
	flags: u8,
}

impl IsrStatusRaw {
	/// Returns a mutable reference to the ISR status capability structure indicated by the
	/// [PciCap](PciCap) struct. Reference has a static lifetime as the structure is controlled by the
	/// device and will not be moved.
	fn map(cap: &PciCap) -> Option<&'static mut IsrStatusRaw> {
		if cap.bar.length < u64::from(cap.length + cap.offset) {
			error!("ISR status config with id {} of device {:x}, does not fit into memeory specified by bar {:x}!",
                cap.id,
                cap.origin.dev_id,
                cap.bar.index
            );
			return None;
		}

		let virt_addr_raw: VirtMemAddr = cap.bar.mem_addr + cap.offset;

		// Create mutable reference to the PCI structure in the devices memory area
		let isr_stat_raw: &mut IsrStatusRaw =
			unsafe { &mut *(usize::from(virt_addr_raw) as *mut IsrStatusRaw) };

		Some(isr_stat_raw)
	}

	// returns true if second bit, from left is 1.
	// read DOES reset flag
	fn cfg_event() -> bool {
		unimplemented!();
	}

	// returns true if first bit, from left is 1.
	// read DOES reset flag
	fn vqueue_event() -> bool {
		unimplemented!();
	}
}

/// PCI configuration access structure of Virtio PCI devices.
/// See Virtio specification v1.1. - 4.1.4.8
///
/// ONLY an alternative access method to the common configuration, notification,
/// ISR and device-specific configuration regions/structures.
//
// Currently has no functionality. All funcitonalty must be done via the read_config methods
// as this struct writes/reads to/from the configuration space which can NOT be mapped!
pub struct PciCfgAlt {
	pci_cap: PciCap,
	pci_cfg_data: [u8; 4], // Data for BAR access
	                       // TODO:
	                       // The fields cap.bar, cap.length, cap.offset and pci_cfg_data are read-write (RW) for the driver.
	                       // To access a device region, the driver writes into the capability structure (ie. within the PCI configuration
	                       // space) as follows:
	                       // • The driver sets the BAR to access by writing to cap.bar.
	                       // • The  driver sets the size of the access by writing 1, 2 or 4 to cap.length.
	                       // • The driver sets the offset within the BAR by writing to cap.offset.
	                       // At that point, pci_cfg_data will provide a window of size cap.length into the given cap.bar at offset cap.offset.
}

impl PciCfgAlt {
	fn new(cap: &PciCap) -> Self {
		PciCfgAlt {
			pci_cap: cap.clone(),
			pci_cfg_data: [0; 4],
		}
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
// Only used for capabilites that require offsets or lengths
// larger than 4GB.
// #[repr(C)]
// struct PciCap64 {
//    pci_cap: PciCap,
//    offset_high: u32,
//    length_high: u32
pub struct ShMemCfg {
	mem_addr: VirtMemAddr,
	length: MemLen,
	sh_mem: ShMem,
	/// Shared memory regions are identified via an ID
	/// See Virtio specification v1.1. - 4.1.4.7
	id: u8,
}

impl ShMemCfg {
	fn new(cap: &PciCap) -> Option<Self> {
		if cap.bar.length < u64::from(cap.length + cap.offset) {
			error!("Shared memory config of with id {} of device {:x}, does not fit into memeory specified by bar {:x}!", 
                cap.id,
                cap.origin.dev_id,
                 cap.bar.index
            );
			return None;
		}

		// Read the PciCap64 fields after the PciCap structure to get the right offset and length

		// Assumes the cap_len is a multiple of 8
		// This read MIGHT be slow, as it does NOT ensure 32 bit alignment.
		let offset_high = env::pci::read_cfg_no_adapter(
			cap.origin.bus,
			cap.origin.bus,
			cap.origin.cfg_ptr + u32::from(cap.origin.cap_struct.cap_len),
		);

		// Create 64 bit offset from high and low 32 bit values
		let offset =
			MemOff::from((u64::from(offset_high) << 32) ^ u64::from(cap.origin.cap_struct.offset));

		// Assumes the cap_len is a multiple of 8
		// This read MIGHT be slow, as it does NOT ensure 32 bit alignment.
		let length_high = env::pci::read_cfg_no_adapter(
			cap.origin.bus,
			cap.origin.bus,
			cap.origin.cfg_ptr + u32::from(cap.origin.cap_struct.cap_len + 4),
		);

		// Create 64 bit length from high and low 32 bit values
		let length =
			MemLen::from((u64::from(length_high) << 32) ^ u64::from(cap.origin.cap_struct.length));

		let virt_addr_raw = cap.bar.mem_addr + offset;
		let raw_ptr = usize::from(virt_addr_raw) as *mut u8;

		// Zero initialize shared memory area
		unsafe {
			for i in 0..usize::from(length) {
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
			length: cap.length,
			sh_mem: ShMem {
				ptr: raw_ptr,
				len: cap.bar.length as usize,
			},
			id: cap.id,
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
// Currently all fields are public as the struct is instanciated in the drivers::virtio::env module
#[derive(Copy, Clone, Debug)]
pub struct PciBar {
	index: u8,
	mem_addr: VirtMemAddr,
	length: u64,
}

impl PciBar {
	pub fn new(index: u8, mem_addr: VirtMemAddr, length: u64) -> Self {
		PciBar {
			index,
			mem_addr,
			length,
		}
	}
}

/// Reads a raw capability struct [PciCapRaw](structs.PcicapRaw.html) out of a PCI device's configuration space.
fn read_cap_raw(adapter: &PciAdapter, register: u32) -> PciCapRaw {
	let mut quadruple_word: [u8; 16] = [0; 16];

	debug!("Converting read word from PCI device config space into native endian bytes.");

	// Write words sequentialy into array
	let mut index = 0;
	for i in 0..4u32 {
		// Read word need to be converted to little endian bytes as PCI is little endian.
		// Intepretation of multi byte values needs to be swapped for big endian machines
		let word: [u8; 4] = env::pci::read_config(adapter, register + 4 * i).to_le_bytes();
		for j in 0..4 {
			quadruple_word[index] = word[j];
			index += 1;
		}
	}

	PciCapRaw {
		cap_vndr: quadruple_word[0],
		cap_next: quadruple_word[1],
		cap_len: quadruple_word[2],
		cfg_type: quadruple_word[3],
		bar_index: quadruple_word[4],
		id: quadruple_word[5],
		// Unwrapping is okay here, as transformed array slice is always 2 * u8 long and initialized
		padding: quadruple_word[6..8].try_into().unwrap(),
		// Unwrapping is okay here, as transformed array slice is always 4 * u8 long and initialized
		offset: u32::from_le_bytes(quadruple_word[8..12].try_into().unwrap()),
		length: u32::from_le_bytes(quadruple_word[12..16].try_into().unwrap()),
	}
}

/// Reads all PCI capabilities, starting at the capabilites list pointer from the
/// PCI device.
///
/// Returns ONLY Virtio specific capabilites, which allow to locate the actual capability
/// structures inside the memory areas, indicated by the BaseAddressRegisters (BAR's).
fn read_caps(adapter: &PciAdapter, bars: Vec<PciBar>) -> Result<Vec<PciCap>, PciError> {
	let ptr: u32 = dev_caps_ptr(adapter);

	// Checks if pointer is well formed and does not point into config header space
	let mut next_ptr = if ptr >= 0x40u32 {
		ptr
	} else {
		return Err(PciError::BadCapPtr(adapter.device_id));
	};

	let mut cap_list: Vec<PciCap> = Vec::new();
	// Loop through capabilties list via next pointer
	'cap_list: while next_ptr != 0u32 {
		// read into raw capabilities structure
		//
		// Devices configuration space muste be read twice
		// and only returns correct values if both reads
		// return equal values.
		// For clarity see Virtio specification v1.1. - 2.4.1
		let mut before = read_cap_raw(adapter, next_ptr);
		let mut cap_raw = read_cap_raw(adapter, next_ptr);

		while before != cap_raw {
			before = read_cap_raw(adapter, next_ptr);
			cap_raw = read_cap_raw(adapter, next_ptr);
		}

		let mut iter = bars.iter();

		// Set next pointer for next iteration of `caplist.
		next_ptr = u32::from(cap_raw.cap_next);

		// Virtio specification v1.1. - 4.1.4 defines virtio specific capability
		// with virtio vendor id = 0x09
		match cap_raw.cap_vndr {
			0x09u8 => {
				let cap_bar: PciBar = loop {
					match iter.next() {
						Some(bar) => {
							// Drivers MUST ignore BAR values different then specified in Virtio spec v1.1. - 4.1.4
							// See Virtio specification v1.1. - 4.1.4.1
							if bar.index <= 5 {
								if bar.index == cap_raw.bar_index {
									// Need to clone here as every PciCap carrys it's bar
									break bar.clone();
								}
							}
						}
						None => {
							error!("Found virtio capability whose BAR is not mapped or non existing. Capability of type {:x} and id {:x} for device {:x}, can not be used!",
                                cap_raw.cfg_type, cap_raw.id, adapter.device_id);

							continue 'cap_list;
						}
					}
				};

				cap_list.push(PciCap {
					cfg_type: CfgType::from(cap_raw.cfg_type),
					bar: cap_bar,
					id: cap_raw.id,
					offset: MemOff::from(cap_raw.offset),
					length: MemLen::from(cap_raw.length),
					origin: Origin {
						cfg_ptr: next_ptr,
						dev: adapter.device,
						bus: adapter.bus,
						dev_id: adapter.device_id,
						cap_struct: cap_raw,
					},
				})
			}
			_ => info!(
				"Non Virtio PCI capability with id {:x} found. And NOT used.",
				cap_raw.cap_vndr
			),
		}
	}

	if cap_list.is_empty() {
		error!(
			"No virtio capability found for device {:x}",
			adapter.device_id
		);
		Err(PciError::NoVirtioCaps(adapter.device_id))
	} else {
		Ok(cap_list)
	}
}

/// Wrapper function to get a devices current status.
/// As the device is not static, return value is not static.
fn dev_status(adapter: &PciAdapter) -> u32 {
	// reads register 01 from PCU Header of type 00H. WHich is the Status(16bit) and Command(16bit) register
	let stat_com_reg = env::pci::read_config(
		adapter,
		u32::from(constants::RegisterHeader00H::PCI_COMMAND_REGISTER),
	);
	stat_com_reg >> 16
}

/// Wrapper function to get a devices capabilites list pointer, which represents
/// an offset starting from the header of the device's configuration space.
fn dev_caps_ptr(adapter: &PciAdapter) -> u32 {
	let cap_lst_reg = env::pci::read_config(
		adapter,
		u32::from(constants::RegisterHeader00H::PCI_CAPABILITY_LIST_REGISTER),
	);
	cap_lst_reg & u32::from(constants::Masks::PCI_MASK_CAPLIST_POINTER)
}

/// Maps memory areas indicated by devices BAR's into virtual address space.
fn map_bars(adapter: &PciAdapter) -> Result<Vec<PciBar>, PciError> {
	crate::drivers::virtio::env::pci::map_bar_mem(adapter)
}

/// Checks if the status of the device inidactes the device is using the
/// capabilites pointer and therefore defines a capabiites list.
fn no_cap_list(adapter: &PciAdapter) -> bool {
	dev_status(adapter) & u32::from(constants::Masks::PCI_MASK_STATUS_CAPABILITIES_LIST) == 0
}

/// Checks if minimal set of capabilities is present.
///
/// INFO: Currently only checks if at least one common config struct has been found and mapped.
fn check_caps(caps: UniCapsColl) -> Result<UniCapsColl, PciError> {
	if caps.com_cfg_list.is_empty() {
		error!("Device with unknwon id, does not have a common config structure!");
		return Err(PciError::General(0));
	}

	Ok(caps)
}

pub fn map_caps(adapter: &PciAdapter) -> Result<UniCapsColl, PciError> {
	// In case caplist pointer is not used, abort as it is essential
	if no_cap_list(adapter) {
		error!("Found virtio device without capability list. Aborting!");
		return Err(PciError::NoCapPtr(adapter.device_id));
	}

	// Mapped memory areas are reachable through PciBar structs.
	let bar_list = match map_bars(adapter) {
		Ok(list) => list,
		Err(pci_error) => return Err(pci_error),
	};

	// Get list of PciCaps pointing to capabilities
	let cap_list = match read_caps(adapter, bar_list) {
		Ok(list) => list,
		Err(pci_error) => return Err(pci_error),
	};

	let mut caps = UniCapsColl::new();
	// Map Caps in virtual memory
	for pci_cap in cap_list {
		match pci_cap.cfg_type {
			CfgType::VIRTIO_PCI_CAP_COMMON_CFG => match ComCfgRaw::map(&pci_cap) {
				Some(cap) => caps.add_cfg_common(ComCfg::new(cap, pci_cap.id)),
				None => error!(
					"Common config capability with id {}, of device {:x}, could not be mapped!",
					pci_cap.id, adapter.device_id
				),
			},
			CfgType::VIRTIO_PCI_CAP_NOTIFY_CFG => match NotifCfg::new(&pci_cap) {
				Some(notif) => caps.add_cfg_notif(notif),
				None => error!(
					"Notification config capability with id {}, of device {:x} could not be used!",
					pci_cap.id, adapter.device_id
				),
			},
			CfgType::VIRTIO_PCI_CAP_ISR_CFG => match IsrStatusRaw::map(&pci_cap) {
				Some(isr_stat) => caps.add_cfg_isr(IsrStatus::new(isr_stat, pci_cap.id)),
				None => error!(
					"ISR status config capability with id {}, of device {:x} could not be used!",
					pci_cap.id, adapter.device_id
				),
			},
			CfgType::VIRTIO_PCI_CAP_PCI_CFG => caps.add_cfg_alt(PciCfgAlt::new(&pci_cap)),
			CfgType::VIRTIO_PCI_CAP_SHARED_MEMORY_CFG => match ShMemCfg::new(&pci_cap) {
				Some(sh_mem) => caps.add_cfg_sh_mem(sh_mem),
				None => error!(
					"Shared Memory config capability with id {}, of device {:x} could not be used!",
					pci_cap.id, adapter.device_id
				),
			},
			CfgType::VIRTIO_PCI_CAP_DEVICE_CFG => caps.add_cfg_dev(pci_cap),

			// PCI's configuration space is allowed to hold other structures, which are not virtio specific and are therefore ignored
			// in the following
			_ => continue,
		}
	}

	check_caps(caps)
}

/// Checks existing drivers for support of given device. Upon match, provides
/// driver with a [Caplist](struct.Caplist.html) struct, holding the structures of the capabilities
/// list of the given device.
pub fn init_device(adapter: &PciAdapter) -> Result<VirtioDriver, DriverError> {
	let virt_drv = match DevId::from(adapter.device_id) {
		DevId::VIRTIO_TRANS_DEV_ID_NET
		| DevId::VIRTIO_TRANS_DEV_ID_BLK
		| DevId::VIRTIO_TRANS_DEV_ID_MEM_BALL
		| DevId::VIRTIO_TRANS_DEV_ID_CONS
		| DevId::VIRTIO_TRANS_DEV_ID_SCSI
		| DevId::VIRTIO_TRANS_DEV_ID_ENTROPY
		| DevId::VIRTIO_TRANS_DEV_ID_9P => {
			warn!(
				"Legacy/transitional Virtio device, with id: 0x{:x} is NOT supported, skipping!",
				adapter.device_id
			);

			// Return Driver error inidacting device is not supported
			Err(DriverError::InitVirtioDevFail(
				VirtioError::DevNotSupported(adapter.device_id),
			))
		}
		DevId::VIRTIO_DEV_ID_NET => match VirtioNetDriver::init(adapter) {
			Ok(virt_net_drv) => {
				info!("Virtio network driver initialized with Virtio network device.");
				Ok(VirtioDriver::Network(virt_net_drv))
			}
			Err(virtio_error) => {
				error!(
					"Virtio networkd driver could not be initialized with device: {:x}",
					adapter.device_id
				);
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		DevId::VIRTIO_DEV_ID_FS => {
			info!("Found Virtio-FS device!");
			// TODO: check subclass
			// TODO: proper error handling on driver creation fail
			match virtio_fs::create_virtiofs_driver(adapter) {
				Some(virt_fs_drv) => {
					kernel_pci::register_driver(PciDriver::VirtioFs(SpinlockIrqSave::new(
						virt_fs_drv,
					)));
					// initialize file system
					virtio_fs::init_fs();
					Ok(VirtioDriver::FileSystem)
				}
				None => Err(DriverError::InitVirtioDevFail(VirtioError::Unknown)),
			}
		}
		_ => {
			warn!(
				"Virtio device with id: 0x{:x} is NOT supported, skipping!",
				adapter.device_id
			);

			// Return Driver error inidacting device is not supported
			Err(DriverError::InitVirtioDevFail(
				VirtioError::DevNotSupported(adapter.device_id),
			))
		}
	};

	match virt_drv {
		Ok(drv) => {
			match &drv {
				VirtioDriver::Network(_) => {
					info!("Install virtio interrupt handler at line {}", adapter.irq);
					// Install interrupt handler
					irq_install_handler(adapter.irq as u32, network_irqhandler as usize);
					add_irq_name(adapter.irq as u32, "virtio_net");

					Ok(drv)
				}
				VirtioDriver::FileSystem => Ok(drv),
			}
		}
		Err(virt_err) => Err(virt_err),
	}
}

pub enum VirtioDriver {
	Network(VirtioNetDriver),
	FileSystem,
}
/// The module contains constants specific to PCI.
#[allow(dead_code)]
pub mod constants {
	// PCI constants
	pub const PCI_MAX_BUS_NUMBER: u8 = 32;
	pub const PCI_MAX_DEVICE_NUMBER: u8 = 32;
	pub const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
	pub const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;
	pub const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;
	pub const PCI_CAP_ID_VNDR_VIRTIO: u32 = 0x09;
	pub const PCI_MASK_IS_DEV_BUS_MASTER: u32 = 0x0000_0004u32;

	/// PCI registers offset inside header,
	/// if PCI header is of type 00h.
	#[allow(dead_code, non_camel_case_types)]
	#[repr(u32)]
	pub enum RegisterHeader00H {
		PCI_ID_REGISTER = 0x00u32,
		PCI_COMMAND_REGISTER = 0x04u32,
		PCI_CLASS_REGISTER = 0x08u32,
		PCI_HEADER_REGISTER = 0x0Cu32,
		PCI_BAR0_REGISTER = 0x10u32,
		PCI_CAPABILITY_LIST_REGISTER = 0x34u32,
		PCI_INTERRUPT_REGISTER = 0x3Cu32,
	}

	impl From<RegisterHeader00H> for u32 {
		fn from(val: RegisterHeader00H) -> u32 {
			match val {
				RegisterHeader00H::PCI_ID_REGISTER => 0x00u32,
				RegisterHeader00H::PCI_COMMAND_REGISTER => 0x04u32,
				RegisterHeader00H::PCI_CLASS_REGISTER => 0x08u32,
				RegisterHeader00H::PCI_HEADER_REGISTER => 0x0Cu32,
				RegisterHeader00H::PCI_BAR0_REGISTER => 0x10u32,
				RegisterHeader00H::PCI_CAPABILITY_LIST_REGISTER => 0x34u32,
				RegisterHeader00H::PCI_INTERRUPT_REGISTER => 0x3Cu32,
			}
		}
	}

	/// PCI masks. For convenience put into an enum and provides
	/// an Into<u32> method for usage.
	#[allow(dead_code, non_camel_case_types)]
	#[repr(u32)]
	pub enum Masks {
		PCI_MASK_IS_BAR_IO_BAR = 0x0000_0001u32,
		PCI_MASK_IS_MEM_BASE_ADDRESS_64BIT = 0x0000_0004u32,
		PCI_MASK_IS_MEM_BAR_PREFETCHABLE = 0x0000_0008u32,
		PCI_MASK_STATUS_CAPABILITIES_LIST = 0x0000_0010u32,
		PCI_MASK_CAPLIST_POINTER = 0x0000_00FCu32,
		PCI_MASK_HEADER_TYPE = 0x007F_0000u32,
		PCI_MASK_MULTIFUNCTION = 0x0080_0000u32,
		PCI_MASK_MEM_BASE_ADDRESS = 0xFFFF_FFF0u32,
		PCI_MASK_IO_BASE_ADDRESS = 0xFFFF_FFFCu32,
	}

	impl From<Masks> for u32 {
		fn from(val: Masks) -> u32 {
			match val {
				Masks::PCI_MASK_STATUS_CAPABILITIES_LIST => 0x0000_0010u32,
				Masks::PCI_MASK_CAPLIST_POINTER => 0x0000_00FCu32,
				Masks::PCI_MASK_HEADER_TYPE => 0x007F_0000u32,
				Masks::PCI_MASK_MULTIFUNCTION => 0x0080_0000u32,
				Masks::PCI_MASK_MEM_BASE_ADDRESS => 0xFFFF_FFF0u32,
				Masks::PCI_MASK_IO_BASE_ADDRESS => 0xFFFF_FFFCu32,
				Masks::PCI_MASK_IS_MEM_BAR_PREFETCHABLE => 0x0000_0008u32,
				Masks::PCI_MASK_IS_MEM_BASE_ADDRESS_64BIT => 0x0000_0004u32,
				Masks::PCI_MASK_IS_BAR_IO_BAR => 0x0000_0001u32,
			}
		}
	}
}
