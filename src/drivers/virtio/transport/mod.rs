//! A module containing virtios transport mechanisms.
//!
//! The module contains only PCI specific transport mechanism.
//! Other mechanisms (MMIO and Channel I/O) are currently not
//! supported.

#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
