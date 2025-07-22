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
		not(all(target_arch = "x86_64", feature = "rtl8139")),
		any(feature = "tcp", feature = "udp")
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
			not(all(target_arch = "x86_64", feature = "rtl8139")),
			any(feature = "tcp", feature = "udp")
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
				VirtioError::FromPci(pci_error) => match pci_error {
					PciError::General(id) => write!(
						f,
						"Driver failed to initialize device with id: {id:#x}. Due to unknown reasosn!"
					),
					PciError::NoBar(id) => write!(
						f,
						"Driver failed to initialize device with id: {id:#x}. Reason: No BAR's found."
					),
					PciError::NoCapPtr(id) => write!(
						f,
						"Driver failed to initialize device with id: {id:#x}. Reason: No Capabilities pointer found."
					),
					PciError::NoVirtioCaps(id) => write!(
						f,
						"Driver failed to initialize device with id: {id:#x}. Reason: No Virtio capabilities were found."
					),
				},
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
					not(all(target_arch = "x86_64", feature = "rtl8139")),
					any(feature = "tcp", feature = "udp")
				))]
				VirtioError::NetDriver(net_error) => match net_error {
					#[cfg(feature = "pci")]
					VirtioNetError::NoDevCfg(id) => write!(
						f,
						"Virtio network driver failed, for device {id:x}, due to a missing or malformed device config!"
					),
					VirtioNetError::FailFeatureNeg(id) => write!(
						f,
						"Virtio network driver failed, for device {id:x}, device did not acknowledge negotiated feature set!"
					),
					VirtioNetError::FeatureRequirementsNotMet(features) => write!(
						f,
						"Virtio network driver tried to set feature bit without setting dependency feature. Feat set: {features:?}"
					),
					VirtioNetError::IncompatibleFeatureSets(driver_features, device_features) => {
						write!(
							f,
							"Feature set: {driver_features:?} , is incompatible with the device features: {device_features:?}"
						)
					}
				},
				#[cfg(feature = "fuse")]
				VirtioError::FsDriver(fs_error) => match fs_error {
					#[cfg(feature = "pci")]
					VirtioFsError::NoDevCfg(id) => write!(
						f,
						"Virtio filesystem driver failed, for device {id:x}, due to a missing or malformed device config!"
					),
					VirtioFsError::FailFeatureNeg(id) => write!(
						f,
						"Virtio filesystem driver failed, for device {id:x}, device did not acknowledge negotiated feature set!"
					),
					VirtioFsError::FeatureRequirementsNotMet(features) => write!(
						f,
						"Virtio filesystem driver tried to set feature bit without setting dependency feature. Feat set: {features:?}"
					),
					VirtioFsError::IncompatibleFeatureSets(driver_features, device_features) => {
						write!(
							f,
							"Feature set: {driver_features:?} , is incompatible with the device features: {device_features:?}",
						)
					}
					VirtioFsError::Unknown => write!(
						f,
						"Virtio filesystem failed, driver failed due unknown reason!"
					),
				},
				#[cfg(feature = "console")]
				VirtioError::ConsoleDriver(console_error) => match console_error {
					#[cfg(feature = "pci")]
					VirtioConsoleError::NoDevCfg(id) => write!(
						f,
						"Virtio console device driver failed, for device {id:x}, due to a missing or malformed device config!"
					),
					VirtioConsoleError::FailFeatureNeg(id) => write!(
						f,
						"Virtio console device driver failed, for device {id:x}, device did not acknowledge negotiated feature set!"
					),
					VirtioConsoleError::FeatureRequirementsNotMet(features) => write!(
						f,
						"Virtio console driver tried to set feature bit without setting dependency feature. Feat set: {features:?}"
					),
					VirtioConsoleError::IncompatibleFeatureSets(
						driver_features,
						device_features,
					) => {
						write!(
							f,
							"Feature set: {driver_features:?} , is incompatible with the device features: {device_features:?}"
						)
					}
				},
				#[cfg(feature = "vsock")]
				VirtioError::VsockDriver(vsock_error) => match vsock_error {
					#[cfg(feature = "pci")]
					VirtioVsockError::NoDevCfg(id) => write!(
						f,
						"Virtio socket device driver failed, for device {id:x}, due to a missing or malformed device config!"
					),
					#[cfg(feature = "pci")]
					VirtioVsockError::NoComCfg(id) => write!(
						f,
						"Virtio socket device driver failed, for device {id:x}, due to a missing or malformed common config!"
					),
					#[cfg(feature = "pci")]
					VirtioVsockError::NoIsrCfg(id) => write!(
						f,
						"Virtio socket device driver failed, for device {id:x}, due to a missing or malformed ISR status config!"
					),
					#[cfg(feature = "pci")]
					VirtioVsockError::NoNotifCfg(id) => write!(
						f,
						"Virtio socket device driver failed, for device {id:x}, due to a missing or malformed notification config!"
					),
					VirtioVsockError::FailFeatureNeg(id) => write!(
						f,
						"Virtio socket device driver failed, for device {id:x}, device did not acknowledge negotiated feature set!"
					),
					VirtioVsockError::FeatureRequirementsNotMet(features) => write!(
						f,
						"Virtio socket driver tried to set feature bit without setting dependency feature. Feat set: {features:?}"
					),
					VirtioVsockError::IncompatibleFeatureSets(driver_features, device_features) => {
						write!(
							f,
							"Feature set: {driver_features:?} , is incompatible with the device features: {device_features:?}"
						)
					}
				},
			}
		}
	}
}
