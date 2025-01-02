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

	use pci_types::MAX_BARS;

	use crate::arch::pci::PciConfigRegion;
	use crate::drivers::pci::PciDevice;
	use crate::drivers::pci::error::PciError;
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
		let mapped_bars: Vec<VirtioPciBar> = (0..u8::try_from(MAX_BARS).unwrap())
			.filter_map(|i| {
				device
					.memory_map_bar(i, true)
					.map(|(addr, size)| (i, addr, size))
			})
			.map(|(i, addr, size)| VirtioPciBar::new(i, addr.as_u64(), size.try_into().unwrap()))
			.collect::<Vec<_>>();

		if mapped_bars.is_empty() {
			let device_id = device.device_id();
			error!("No correct memory BAR for device {:x} found.", device_id);
			Err(PciError::NoBar(device_id))
		} else {
			Ok(mapped_bars)
		}
	}
}
