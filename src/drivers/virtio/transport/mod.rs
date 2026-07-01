#[cfg(not(feature = "pci"))]
use mmio::IsrStatus;
#[cfg(feature = "pci")]
use pci::IsrStatus;

#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;

pub(crate) enum InterruptCapability {
	IsrStatus(IsrStatus),
	#[cfg(all(feature = "pci", target_arch = "x86_64"))]
	Msix(volatile::VolatileRef<'static, [crate::drivers::pci::msix::TableEntry]>),
}
