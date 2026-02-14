//! Virtio infrastructure.
//!
//! This module provides [`transport`] infrastructure as well as [`virtqueue`] infrastructure.

#![cfg_attr(
	not(any(
		feature = "virtio-console",
		feature = "virtio-fs",
		feature = "virtio-net",
		feature = "virtio-vsock"
	)),
	allow(dead_code)
)]

pub mod transport;
pub mod virtqueue;

use core::fmt;

use virtio::FeatureBits;

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
	use core::{array, mem};

	use virtio::{le32, le128};
	use volatile::VolatilePtr;
	use volatile::access::ReadWrite;

	pub trait ControlRegistersAccess<'a>: Sized + Copy {
		fn read_device_feature_word(self, i: u32) -> le32;
		fn write_driver_feature_word(self, i: u32, word: le32);

		fn read_device_features(self) -> virtio::F {
			let features = array::from_fn(|i| {
				let i = u32::try_from(i).unwrap();
				self.read_device_feature_word(i)
			});

			let features = unsafe { mem::transmute::<[le32; 4], le128>(features) };

			virtio::F::from_bits_retain(features)
		}

		fn write_driver_features(self, features: virtio::F) {
			let features = features.bits();

			let features = unsafe { mem::transmute::<le128, [le32; 4]>(features) };

			for (i, word) in features.into_iter().enumerate() {
				let i = u32::try_from(i).unwrap();
				self.write_driver_feature_word(i, word);
			}
		}
	}

	#[cfg(feature = "pci")]
	impl<'a> ControlRegistersAccess<'a> for VolatilePtr<'a, virtio::pci::CommonCfg, ReadWrite> {
		fn read_device_feature_word(self, i: u32) -> le32 {
			use virtio::pci::CommonCfgVolatileFieldAccess;

			self.device_feature_select().write(i.into());
			self.device_feature().read()
		}

		fn write_driver_feature_word(self, i: u32, word: le32) {
			use virtio::pci::CommonCfgVolatileFieldAccess;

			self.driver_feature_select().write(i.into());
			self.driver_feature().write(word);
		}
	}

	#[cfg(not(feature = "pci"))]
	impl<'a> ControlRegistersAccess<'a> for VolatilePtr<'a, virtio::mmio::DeviceRegisters, ReadWrite> {
		fn read_device_feature_word(self, i: u32) -> le32 {
			use virtio::mmio::DeviceRegistersVolatileFieldAccess;

			// QEMU only supports index 0 and 1 for virtio-mmio:
			// https://gitlab.com/qemu-project/qemu/-/blob/v10.2.0/hw/virtio/virtio-mmio.c#L305-311
			if i > 1 {
				return 0.into();
			}

			self.device_features_sel().write(i.into());
			self.device_features().read()
		}

		fn write_driver_feature_word(self, i: u32, word: le32) {
			use virtio::mmio::DeviceRegistersVolatileFieldAccess;

			// QEMU only supports index 0 and 1 for virtio-mmio:
			// https://gitlab.com/qemu-project/qemu/-/blob/v10.2.0/hw/virtio/virtio-mmio.c#L326-332
			if i > 1 {
				debug_assert!(word.to_ne() == 0);
				return;
			}

			self.driver_features_sel().write(i.into());
			self.driver_features().write(word);
		}
	}
}

pub trait ControlRegisters<'a>: self::control_registers_access::ControlRegistersAccess<'a> {
	fn negotiate_features<DF>(self, driver_features: DF) -> DF
	where
		DF: FeatureBits + From<virtio::F> + AsRef<virtio::F> + AsMut<virtio::F> + fmt::Debug + Copy,
		virtio::F: From<DF> + AsRef<DF> + AsMut<DF>;
}

impl<'a, T> ControlRegisters<'a> for T
where
	T: self::control_registers_access::ControlRegistersAccess<'a>,
{
	fn negotiate_features<DF>(self, driver_features: DF) -> DF
	where
		DF: FeatureBits + From<virtio::F> + AsRef<virtio::F> + AsMut<virtio::F> + fmt::Debug + Copy,
		virtio::F: From<DF> + AsRef<DF> + AsMut<DF>,
	{
		let device_features = DF::from(self.read_device_features());
		info!("device_features = {device_features:?}");
		debug_assert!(
			device_features.requirements_satisfied(),
			"The device offers a feature which requires another feature which was not offered."
		);

		info!("driver_features = {driver_features:?}");
		debug_assert!(
			driver_features.requirements_satisfied(),
			"The driver offers a feature which requires another feature which was not offered.",
		);

		let common_features = device_features.intersection(driver_features);
		info!("common_features = {common_features:?}");
		// This should be logically unreachable.
		debug_assert!(
			common_features.requirements_satisfied(),
			"We negotiated a feature which requires another feature which was not negotiated."
		);

		self.write_driver_features(common_features.into());

		common_features
	}
}

pub mod error {
	use thiserror::Error;

	#[cfg(feature = "virtio-console")]
	pub use crate::drivers::console::error::VirtioConsoleError;
	#[cfg(feature = "virtio-fs")]
	pub use crate::drivers::fs::error::VirtioFsInitError;
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
		FsDriver(VirtioFsInitError),

		#[cfg(feature = "virtio-vsock")]
		#[error(transparent)]
		VsockDriver(VirtioVsockError),

		#[cfg(feature = "virtio-console")]
		#[error(transparent)]
		ConsoleDriver(VirtioConsoleError),
	}
}
