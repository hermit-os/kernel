//! A module containing all virtio specific pci functionality
//!
//! The module contains ...
#![allow(dead_code)]

#[cfg(feature = "console")]
use alloc::boxed::Box;
use core::mem;

use memory_addresses::PhysAddr;
use virtio::mmio::{
	DeviceRegisters, DeviceRegistersVolatileFieldAccess, DeviceRegistersVolatileWideFieldAccess,
	InterruptStatus, NotificationData,
};
use virtio::{DeviceStatus, le32};
use volatile::access::ReadOnly;
use volatile::{VolatilePtr, VolatileRef};

use crate::drivers::InterruptLine;
#[cfg(feature = "console")]
use crate::drivers::console::VirtioConsoleDriver;
use crate::drivers::error::DriverError;
#[cfg(feature = "virtio-net")]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::virtio::error::VirtioError;

pub struct VqCfgHandler<'a> {
	vq_index: u16,
	raw: VolatileRef<'a, DeviceRegisters>,
}

impl VqCfgHandler<'_> {
	// TODO: Create type for queue selected invariant to get rid of `self.select_queue()` everywhere.
	fn select_queue(&mut self) {
		self.raw
			.as_mut_ptr()
			.queue_sel()
			.write(self.vq_index.into());
	}

	/// Sets the size of a given virtqueue. In case the provided size exceeds the maximum allowed
	/// size, the size is set to this maximum instead. Else size is set to the provided value.
	///
	/// Returns the set size in form of a `u16`.
	pub fn set_vq_size(&mut self, size: u16) -> u16 {
		self.select_queue();
		let ptr = self.raw.as_mut_ptr();

		let num_max = ptr.queue_num_max().read().to_ne();
		let size = size.min(num_max);
		ptr.queue_num().write(size.into());
		size
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

	pub fn enable_queue(&mut self) {
		self.select_queue();

		self.raw.as_mut_ptr().queue_ready().write(true);
	}
}

/// Wraps a [MmioRegisterLayout] in order to preserve
/// the original structure.
///
/// Provides a safe API for the raw structure and allows interaction with the device via
/// the structure.
pub struct ComCfg {
	// FIXME: remove 'static lifetime
	com_cfg: VolatileRef<'static, DeviceRegisters>,

	/// Preferences of the device for this config. From 1 (highest) to 2^7-1 (lowest)
	rank: u8,
}

// Public Interface of ComCfg
impl ComCfg {
	pub fn new(raw: VolatileRef<'static, DeviceRegisters>, rank: u8) -> Self {
		ComCfg { com_cfg: raw, rank }
	}

	pub fn device_config_space(&self) -> VolatilePtr<'_, DeviceRegisters, ReadOnly> {
		self.com_cfg.as_ptr()
	}

	/// Select a queue via an index. If queue does NOT exist returns `None`, else
	/// returns `Some(VqCfgHandler)`.
	///
	/// INFO: The queue size is automatically bounded by constant `src::config:VIRTIO_MAX_QUEUE_SIZE`.
	pub fn select_vq(&mut self, index: u16) -> Option<VqCfgHandler<'_>> {
		if self.get_max_queue_size(index) == 0 {
			None
		} else {
			Some(VqCfgHandler {
				vq_index: index,
				raw: self.com_cfg.borrow_mut(),
			})
		}
	}

	pub fn get_max_queue_size(&mut self, sel: u16) -> u16 {
		let ptr = self.com_cfg.as_mut_ptr();
		ptr.queue_sel().write(sel.into());
		ptr.queue_num_max().read().to_ne()
	}

	pub fn get_queue_ready(&mut self, sel: u16) -> bool {
		let ptr = self.com_cfg.as_mut_ptr();
		ptr.queue_sel().write(sel.into());
		ptr.queue_ready().read()
	}

	/// Returns the device status field.
	pub fn dev_status(&self) -> u8 {
		self.com_cfg.as_ptr().status().read().bits()
	}

	/// Resets the device status field to zero.
	pub fn reset_dev(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.status()
			.write(DeviceStatus::empty());
	}

	/// Sets the device status field to FAILED.
	/// A driver MUST NOT initialize and use the device any further after this.
	/// A driver MAY use the device again after a proper reset of the device.
	pub fn set_failed(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.status()
			.write(DeviceStatus::FAILED);
	}

	/// Sets the ACKNOWLEDGE bit in the device status field. This indicates, the
	/// OS has notived the device
	pub fn ack_dev(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.status()
			.update(|status| status | DeviceStatus::ACKNOWLEDGE);
	}

	/// Sets the DRIVER bit in the device status field. This indicates, the OS
	/// know how to run this device.
	pub fn set_drv(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.status()
			.update(|status| status | DeviceStatus::DRIVER);
	}

	/// Sets the FEATURES_OK bit in the device status field.
	///
	/// Drivers MUST NOT accept new features after this step.
	pub fn features_ok(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.status()
			.update(|status| status | DeviceStatus::FEATURES_OK);
	}

	/// In order to correctly check feature negotiaten, this function
	/// MUST be called after [self.features_ok()](ComCfg::features_ok()) in order to check
	/// if features have been accepted by the device after negotiation.
	///
	/// Re-reads device status to ensure the FEATURES_OK bit is still set:
	/// otherwise, the device does not support our subset of features and the device is unusable.
	pub fn check_features(&self) -> bool {
		self.com_cfg
			.as_ptr()
			.status()
			.read()
			.contains(DeviceStatus::FEATURES_OK)
	}

	/// Sets the DRIVER_OK bit in the device status field.
	///
	/// After this call, the device is "live"!
	pub fn drv_ok(&mut self) {
		self.com_cfg
			.as_mut_ptr()
			.status()
			.update(|status| status | DeviceStatus::DRIVER_OK);
	}

	/// Returns the features offered by the device.
	pub fn dev_features(&mut self) -> virtio::F {
		let ptr = self.com_cfg.as_mut_ptr();

		// Indicate device to show high 32 bits in device_feature field.
		// See Virtio specification v1.1. - 4.1.4.3
		ptr.device_features_sel().write(1.into());

		// read high 32 bits of device features
		let mut device_features = u64::from(ptr.device_features().read().to_ne()) << 32;

		// Indicate device to show low 32 bits in device_feature field.
		// See Virtio specification v1.1. - 4.1.4.3
		ptr.device_features_sel().write(0.into());

		// read low 32 bits of device features
		device_features |= u64::from(ptr.device_features().read().to_ne());

		virtio::F::from_bits_retain(u128::from(device_features).into())
	}

	/// Write selected features into driver_select field.
	pub fn set_drv_features(&mut self, features: virtio::F) {
		let ptr = self.com_cfg.as_mut_ptr();

		let features = features.bits().to_ne() as u64;
		let high: u32 = (features >> 32) as u32;
		let low: u32 = features as u32;

		// Indicate to device that driver_features field shows low 32 bits.
		// See Virtio specification v1.1. - 4.1.4.3
		ptr.driver_features_sel().write(0.into());

		// write low 32 bits of device features
		ptr.driver_features().write(low.into());

		// Indicate to device that driver_features field shows high 32 bits.
		// See Virtio specification v1.1. - 4.1.4.3
		ptr.driver_features_sel().write(1.into());

		// write high 32 bits of device features
		ptr.driver_features().write(high.into());
	}

	pub fn print_information(&mut self) {
		let ptr = self.com_cfg.as_ptr();

		infoheader!(" MMIO REGISTER LAYOUT INFORMATION ");

		infoentry!("Device version", "{:#X}", ptr.version().read());
		infoentry!("Device ID", "{:?}", ptr.device_id().read());
		infoentry!("Vendor ID", "{:#X}", ptr.vendor_id().read());
		infoentry!("Device Features", "{:#X}", self.dev_features());
		let ptr = self.com_cfg.as_ptr();
		infoentry!("Interrupt status", "{:#X}", ptr.interrupt_status().read());
		infoentry!("Device status", "{:#X}", ptr.status().read());

		infofooter!();
	}
}

/// Notification Structure to handle virtqueue notification settings.
/// See Virtio specification v1.1 - 4.1.4.4
pub struct NotifCfg {
	/// Start addr, from where the notification addresses for the virtqueues are computed
	queue_notify: *mut le32,
}

// FIXME: make `queue_notify` implement `Send` instead
unsafe impl Send for NotifCfg {}

impl NotifCfg {
	pub fn new(mut registers: VolatileRef<'_, DeviceRegisters>) -> Self {
		let raw = registers.as_mut_ptr().queue_notify().as_raw_ptr().as_ptr();

		NotifCfg { queue_notify: raw }
	}

	pub fn notification_location(&self, _vq_cfg_handler: &mut VqCfgHandler<'_>) -> *mut le32 {
		self.queue_notify
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
		let notification_data = if self.f_notif_data {
			data.into_bits()
		} else {
			u32::from(data.vqn()).into()
		};

		unsafe {
			self.notif_addr.write_volatile(notification_data);
		}
	}
}

/// Wraps a [`DeviceRegisters`] in order to preserve
/// the original structure and allow interaction with the device via
/// the structure.
///
/// Provides a safe API for Raw structure and allows interaction with the device via
/// the structure.
pub struct IsrStatus {
	// FIXME: integrate into device register struct
	raw: VolatileRef<'static, DeviceRegisters>,
}

impl IsrStatus {
	pub fn new(registers: VolatileRef<'_, DeviceRegisters>) -> Self {
		let raw =
			unsafe { mem::transmute::<VolatileRef<'_, _>, VolatileRef<'static, _>>(registers) };
		Self { raw }
	}

	pub fn is_queue_interrupt(&self) -> InterruptStatus {
		self.raw.as_ptr().interrupt_status().read()
	}

	pub fn acknowledge(&mut self) {
		let ptr = self.raw.as_mut_ptr();
		let status = ptr.interrupt_status().read();
		ptr.interrupt_ack().write(status);
	}
}

pub(crate) enum VirtioDriver {
	#[cfg(feature = "virtio-net")]
	Network(VirtioNetDriver),
	#[cfg(feature = "console")]
	Console(Box<VirtioConsoleDriver>),
}

#[allow(unused_variables)]
pub(crate) fn init_device(
	registers: VolatileRef<'static, DeviceRegisters>,
	irq_no: InterruptLine,
) -> Result<VirtioDriver, DriverError> {
	let dev_id: u16 = 0;

	if registers.as_ptr().version().read().to_ne() == 0x1 {
		error!("Legacy interface isn't supported!");
		return Err(DriverError::InitVirtioDevFail(
			VirtioError::DevNotSupported(dev_id),
		));
	}

	// Verify the device-ID to find the network card
	match registers.as_ptr().device_id().read() {
		#[cfg(feature = "virtio-net")]
		virtio::Id::Net => match VirtioNetDriver::init(dev_id, registers, irq_no) {
			Ok(virt_net_drv) => {
				info!("Virtio network driver initialized.");

				crate::arch::interrupts::add_irq_name(irq_no, "virtio");
				info!("Virtio interrupt handler at line {irq_no}");

				Ok(VirtioDriver::Network(virt_net_drv))
			}
			Err(virtio_error) => {
				error!("Virtio network driver could not be initialized with device");
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		#[cfg(feature = "console")]
		virtio::Id::Console => match VirtioConsoleDriver::init(dev_id, registers, irq_no) {
			Ok(virt_console_drv) => {
				info!("Virtio console driver initialized.");

				crate::arch::interrupts::add_irq_name(irq_no, "virtio");
				info!("Virtio interrupt handler at line {irq_no}");

				Ok(VirtioDriver::Console(Box::new(virt_console_drv)))
			}
			Err(virtio_error) => {
				error!("Virtio console driver could not be initialized with device");
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		#[cfg(feature = "vsock")]
		virtio::Id::Vsock => match VirtioVsockDriver::init(dev_id, registers, irq_no) {
			Ok(virt_net_drv) => {
				info!("Virtio sock driver initialized.");

				crate::arch::interrupts::add_irq_name(irq_no, "virtio");
				info!("Virtio interrupt handler at line {irq_no}");

				Ok(VirtioDriver::Vsock(virt_vsock_drv))
			}
			Err(virtio_error) => {
				error!("Virtio sock driver could not be initialized with device");
				Err(DriverError::InitVirtioDevFail(virtio_error))
			}
		},
		device_id => {
			error!("Device with id {device_id:?} is currently not supported!");
			// Return Driver error inidacting device is not supported
			Err(DriverError::InitVirtioDevFail(
				VirtioError::DevNotSupported(dev_id),
			))
		}
	}
}
