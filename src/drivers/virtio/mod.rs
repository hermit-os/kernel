//! A module containing virtios core infrastructure for hermit-rs.
//!
//! The module contains virtios transport mechanisms, virtqueues and virtio specific errors
pub mod transport;
pub mod virtqueue;

pub mod error {
	use core::fmt;

	#[cfg(feature = "console")]
	pub use crate::drivers::console::error::VirtioConsoleError;
	#[cfg(feature = "fuse")]
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
	#[derive(Debug)]
	pub enum VirtioError {
		#[cfg(feature = "pci")]
		FromPci(PciError),
		#[cfg(feature = "pci")]
		NoComCfg(u16),
		#[cfg(feature = "pci")]
		NoIsrCfg(u16),
		#[cfg(feature = "pci")]
		NoNotifCfg(u16),
		DevNotSupported(u16),
		#[cfg(all(
			not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
			not(feature = "rtl8139"),
			feature = "virtio-net",
		))]
		NetDriver(VirtioNetError),
		#[cfg(feature = "fuse")]
		FsDriver(VirtioFsError),
		#[cfg(feature = "vsock")]
		VsockDriver(VirtioVsockError),
		#[cfg(feature = "console")]
		ConsoleDriver(VirtioConsoleError),
		#[cfg(not(feature = "pci"))]
		Unknown,
	}

	impl fmt::Display for VirtioError {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			match self {
				#[cfg(not(feature = "pci"))]
				VirtioError::Unknown => write!(f, "Driver failure"),
				#[cfg(feature = "pci")]
				VirtioError::FromPci(pci_error) => pci_error.fmt(f),
				#[cfg(feature = "pci")]
				VirtioError::NoComCfg(id) => write!(
					f,
					"Virtio driver failed, for device {id:x}, due to a missing or malformed common config!"
				),
				#[cfg(feature = "pci")]
				VirtioError::NoIsrCfg(id) => write!(
					f,
					"Virtio driver failed, for device {id:x}, due to a missing or malformed ISR status config!"
				),
				#[cfg(feature = "pci")]
				VirtioError::NoNotifCfg(id) => write!(
					f,
					"Virtio driver failed, for device {id:x}, due to a missing or malformed notification config!"
				),
				VirtioError::DevNotSupported(id) => {
					write!(f, "Device with id {id:#x} not supported.")
				}
				#[cfg(all(
					not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
					not(feature = "rtl8139"),
					feature = "virtio-net",
				))]
				VirtioError::NetDriver(net_error) => net_error.fmt(f),
				#[cfg(feature = "fuse")]
				VirtioError::FsDriver(fs_error) => fs_error.fmt(f),
				#[cfg(feature = "console")]
				VirtioError::ConsoleDriver(console_error) => console_error.fmt(f),
				#[cfg(feature = "vsock")]
				VirtioError::VsockDriver(vsock_error) => vsock_error.fmt(f),
			}
		}
	}
}
