//! A module containing virtios transport mechanisms.
//!
//! The module contains only PCI specific transport mechanism.
//! Other mechanisms (MMIO and Channel I/O) are currently not
//! supported.

#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;

#[cfg(all(
	any(feature = "vsock", feature = "tcp", feature = "udp"),
	not(feature = "pci")
))]
use crate::arch::kernel::mmio as hardware;
#[cfg(all(
	any(feature = "vsock", feature = "tcp", feature = "udp", feature = "fuse"),
	feature = "pci"
))]
use crate::drivers::pci as hardware;
