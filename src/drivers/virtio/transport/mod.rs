use core::ops::BitAnd;

use memory_addresses::PhysAddr;
use virtio::{DeviceConfigSpace, le32};

#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;

pub trait Transport: Sized {
	type ComCfg: ComCfg<Self>;
	type NotifCfg: NotifCfg<Self>;
	type IsrStatus: IsrStatus;
	type VqCfgHandler<'a>: VqCfgHandler;
	type NotifCtrl: NotifCtrl + Send;
}

pub trait ComCfg<T: Transport> {
	fn reset_dev(&mut self);
	fn ack_dev(&mut self);
	fn set_drv(&mut self);
	fn control_registers(&mut self) -> impl super::ControlRegisters<'_>;
	fn features_ok(&mut self);
	fn check_features(&self) -> bool;
	fn select_vq(&mut self, index: u16) -> Option<T::VqCfgHandler<'_>>;
	fn drv_ok(&mut self);
	#[allow(dead_code)]
	fn set_failed(&mut self);
	#[allow(dead_code)]
	fn device_config_space(&self) -> impl DeviceConfigSpace;
}

pub trait NotifCfg<T: Transport> {
	fn notification_location(&self, vq_cfg_handler: &mut T::VqCfgHandler<'_>) -> *mut le32;
}

pub trait IsrStatus {
	type Status: BitAnd<Output = Self::Status> + PartialEq + Copy;
	const CONFIGURATION_CHANGE: Self::Status;

	fn acknowledge(&mut self) -> Self::Status;
}

pub trait VqCfgHandler {
	fn enable_queue(&mut self);
	fn set_dev_ctrl_addr(&mut self, addr: PhysAddr);
	fn set_drv_ctrl_addr(&mut self, addr: PhysAddr);
	fn set_ring_addr(&mut self, addr: PhysAddr);
	fn set_vq_size(&mut self, max_size: u16) -> u16;
}

pub trait NotifCtrl {
	type NotificationData: NotificationData;

	fn new(notif_addr: *mut le32) -> Self;
	fn notify_dev(&self, data: Self::NotificationData);
	fn enable_notif_data(&mut self);
}

pub trait NotificationData {
	fn new() -> Self;
	fn with_next_idx(self, value: u16) -> Self;
	fn with_next_off(self, value: u16) -> Self;
	fn with_next_wrap(self, value: u8) -> Self;
	fn with_vqn(self, value: u16) -> Self;
}
