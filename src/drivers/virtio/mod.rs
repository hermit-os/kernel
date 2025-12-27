//! Virtio infrastructure.
//!
//! This module provides [`transport`] infrastructure as well as [`virtqueue`] infrastructure.

use core::{array, mem};

use virtio::{FeatureBits, le32, le128};

pub mod transport;
pub mod virtqueue;

trait VirtioIdExt {
	fn as_feature(&self) -> Option<&str>;
}

impl VirtioIdExt for virtio::Id {
	fn as_feature(&self) -> Option<&str> {
		let feature = match self {
			Self::Net => "virtio-net",
			Self::Console => "virtio-console",
			Self::Fs => "virtio-fs",
			Self::Vsock => "virtio-vsock",
			_ => return None,
		};

		Some(feature)
	}
}

mod control_registers_access {
	use virtio::le32;
	use virtio::mmio::DeviceRegistersVolatileFieldAccess;
	use virtio::pci::CommonCfgVolatileFieldAccess;
	use volatile::VolatilePtr;
	use volatile::access::{ReadOnly, ReadWrite, WriteOnly};

	pub trait ControlRegistersAccess<'a> {
		fn device_features_select_ptr(self) -> VolatilePtr<'a, le32, WriteOnly>;
		fn device_features_ptr(self) -> VolatilePtr<'a, le32, ReadOnly>;
		fn driver_features_select_ptr(self) -> VolatilePtr<'a, le32, WriteOnly>;
		fn driver_features_ptr(self) -> VolatilePtr<'a, le32, WriteOnly>;
	}

	impl<'a> ControlRegistersAccess<'a> for VolatilePtr<'a, virtio::pci::CommonCfg, ReadWrite> {
		fn device_features_select_ptr(self) -> VolatilePtr<'a, le32, WriteOnly> {
			CommonCfgVolatileFieldAccess::device_feature_select(self).restrict()
		}

		fn device_features_ptr(self) -> VolatilePtr<'a, le32, ReadOnly> {
			CommonCfgVolatileFieldAccess::device_feature(self)
		}

		fn driver_features_select_ptr(self) -> VolatilePtr<'a, le32, WriteOnly> {
			CommonCfgVolatileFieldAccess::driver_feature_select(self).restrict()
		}

		fn driver_features_ptr(self) -> VolatilePtr<'a, le32, WriteOnly> {
			CommonCfgVolatileFieldAccess::driver_feature(self).restrict()
		}
	}

	impl<'a> ControlRegistersAccess<'a> for VolatilePtr<'a, virtio::mmio::DeviceRegisters, ReadWrite> {
		fn device_features_select_ptr(self) -> VolatilePtr<'a, le32, WriteOnly> {
			DeviceRegistersVolatileFieldAccess::device_features_sel(self)
		}

		fn device_features_ptr(self) -> VolatilePtr<'a, le32, ReadOnly> {
			DeviceRegistersVolatileFieldAccess::device_features(self)
		}

		fn driver_features_select_ptr(self) -> VolatilePtr<'a, le32, WriteOnly> {
			DeviceRegistersVolatileFieldAccess::driver_features_sel(self)
		}

		fn driver_features_ptr(self) -> VolatilePtr<'a, le32, WriteOnly> {
			DeviceRegistersVolatileFieldAccess::driver_features(self)
		}
	}
}

pub trait ControlRegisters<'a>:
	self::control_registers_access::ControlRegistersAccess<'a> + Sized + Copy
{
	fn read_device_features(self) -> virtio::F;
	fn write_driver_features(self, features: virtio::F);
	fn negotiate_features(self, driver_features: virtio::F) -> virtio::F;
}

impl<'a, T> ControlRegisters<'a> for T
where
	T: self::control_registers_access::ControlRegistersAccess<'a> + Sized + Copy,
{
	fn read_device_features(self) -> virtio::F {
		let features = array::from_fn(|i| {
			let i = u32::try_from(i).unwrap();
			self.device_features_select_ptr().write(i.into());
			self.device_features_ptr().read()
		});

		let features = unsafe { mem::transmute::<[le32; 4], le128>(features) };

		virtio::F::from_bits_retain(features)
	}

	fn write_driver_features(self, features: virtio::F) {
		let features = features.bits();

		let features = unsafe { mem::transmute::<le128, [le32; 4]>(features) };

		for (i, features) in features.into_iter().enumerate() {
			let i = u32::try_from(i).unwrap();
			self.driver_features_select_ptr().write(i.into());
			self.driver_features_ptr().write(features);
		}
	}

	fn negotiate_features(self, driver_features: virtio::F) -> virtio::F {
		let device_features = self.read_device_features();
		debug!("device_features = {device_features:?}");
		debug_assert!(
			device_features.requirements_satisfied(),
			"The device offers a feature which requires another feature which was not offered."
		);

		debug!("driver_features = {driver_features:?}");
		debug_assert!(
			driver_features.requirements_satisfied(),
			"The driver offers a feature which requires another feature which was not offered.",
		);

		let common_features = device_features & driver_features;
		debug!("common_features = {common_features:?}");
		// This should be logically unreachable.
		debug_assert!(
			common_features.requirements_satisfied(),
			"We negotiated a feature which requires another feature which was not negotiated."
		);

		self.write_driver_features(common_features);

		common_features
	}
}

pub mod error {
	use thiserror::Error;

	#[cfg(feature = "virtio-console")]
	pub use crate::drivers::console::error::VirtioConsoleError;
	#[cfg(feature = "virtio-fs")]
	pub use crate::drivers::fs::virtio_fs::error::VirtioFsError;
	#[cfg(all(
		not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
		not(feature = "rtl8139"),
		feature = "virtio-net",
	))]
	pub use crate::drivers::net::virtio::error::VirtioNetError;
	#[cfg(feature = "pci")]
	use crate::drivers::pci::error::PciError;
	#[cfg(feature = "virtio-vsock")]
	pub use crate::drivers::vsock::error::VirtioVsockError;

	#[allow(dead_code)]
	#[derive(Error, Debug)]
	pub enum VirtioError {
		#[cfg(feature = "pci")]
		#[error(transparent)]
		FromPci(PciError),

		#[cfg(feature = "pci")]
		#[error(
			"Virtio driver failed, for device {0:x}, due to a missing or malformed common config!"
		)]
		NoComCfg(u16),

		#[cfg(feature = "pci")]
		#[error(
			"Virtio driver failed, for device {0:x}, due to a missing or malformed ISR status config!"
		)]
		NoIsrCfg(u16),

		#[cfg(feature = "pci")]
		#[error(
			"Virtio driver failed, for device {0:x}, due to a missing or malformed notification config!"
		)]
		NoNotifCfg(u16),

		#[error("Device with id {0:#x} not supported.")]
		DevNotSupported(u16),

		#[cfg(all(
			not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
			not(feature = "rtl8139"),
			feature = "virtio-net",
		))]
		#[error(transparent)]
		NetDriver(VirtioNetError),

		#[cfg(feature = "virtio-fs")]
		#[error(transparent)]
		FsDriver(VirtioFsError),

		#[cfg(feature = "virtio-vsock")]
		#[error(transparent)]
		VsockDriver(VirtioVsockError),

		#[cfg(feature = "virtio-console")]
		#[error(transparent)]
		ConsoleDriver(VirtioConsoleError),

		#[cfg(not(feature = "pci"))]
		#[error("Driver failure")]
		Unknown,
	}
}
