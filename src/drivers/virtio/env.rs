//! A module containing all environment specific function calls.
//!
//! The module should easy partability of the code. Furthermore it provides
//! a clean boundary between virtio and the rest of the kernel. One additional aspect is to
//! ensure only a single location needs changes, in cases where the underlying kernel code is changed

/// This module is used as a single entry point from Virtio code into
/// other parts of the kernel.
///
/// INFO: Values passed on to PCI devices are automatically converted into little endian
/// coding. Values provided from PCI devices are passed as native endian values.
/// Meaning they are converted into big endian values on big endian machines and
/// are not changed on little endian machines.
#[cfg(feature = "pci")]
pub mod pci {
	use alloc::vec::Vec;

	use pci_types::{Bar, MAX_BARS};

	use crate::arch::mm::PhysAddr;
	use crate::arch::pci::PciConfigRegion;
	use crate::drivers::pci::error::PciError;
	use crate::drivers::pci::PciDevice;
	use crate::drivers::virtio::transport::pci::PciBar as VirtioPciBar;

	/// Maps all memory areas indicated by the devices BAR's into
	/// Virtual address space.
	///
	/// As this function uses parts of the kernel pci code it is
	/// outsourced into the env::pci module.
	///
	/// WARN: Currently unsafely casts kernel::PciBar.size (usize) to an
	/// u64
	pub(crate) fn map_bar_mem(
		device: &PciDevice<PciConfigRegion>,
	) -> Result<Vec<VirtioPciBar>, PciError> {
		let mut mapped_bars: Vec<VirtioPciBar> = Vec::new();

		for i in 0..MAX_BARS {
			match device.get_bar(i.try_into().unwrap()) {
				Some(Bar::Io { .. }) => {
					warn!("Cannot map I/O BAR!");
					continue;
				}
				Some(Bar::Memory64 {
					address,
					size,
					prefetchable,
				}) => {
					if !prefetchable {
						warn!("Currently only mapping of prefetchable BAR's is supported!");
						continue;
					}

					let virtual_address = crate::mm::map(
						PhysAddr::from(address),
						size.try_into().unwrap(),
						true,
						true,
						true,
					)
					.0;

					mapped_bars.push(VirtioPciBar::new(
						i.try_into().unwrap(),
						virtual_address,
						size,
					));
				}
				Some(Bar::Memory32 { .. }) => {
					warn!("Currently only mapping of 64 bit BAR's is supported!");
				}
				_ => {}
			}
		}

		if mapped_bars.is_empty() {
			let device_id = device.device_id();
			error!("No correct memory BAR for device {:x} found.", device_id);
			Err(PciError::NoBar(device_id))
		} else {
			Ok(mapped_bars)
		}
	}
}
