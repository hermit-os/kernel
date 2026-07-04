#[cfg(not(feature = "pci"))]
use mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use pci::{ComCfg, IsrStatus, NotifCfg};

#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;

pub(crate) enum InterruptCapability {
	IsrStatus(IsrStatus),
	#[cfg(all(feature = "pci", target_arch = "x86_64"))]
	Msix(volatile::VolatileRef<'static, [crate::drivers::pci::msix::TableEntry]>),
}

/// Universal Caplist Collections holds all universal capability structures for
/// a given Virtio device.
pub struct UniCapsColl {
	pub(crate) com_cfg: ComCfg,
	pub(crate) notif_cfg: NotifCfg,
	pub(crate) int_cap: InterruptCapability,
}
