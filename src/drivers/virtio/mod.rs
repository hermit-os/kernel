//! Virtio infrastructure.
//!
//! This module provides [`transport`] infrastructure as well as [`virtqueue`] infrastructure.

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
	#[cfg(feature = "vsock")]
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

		#[cfg(feature = "vsock")]
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
