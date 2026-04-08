use num_enum::{IntoPrimitive, TryFromPrimitive};

/// A memory map entry.
///
/// This entry is part of the start info's memory map that describes physical memory.
pub struct MemmapEntry {
	/// The physical address of this memory map entry.
	pub phys_addr: usize,

	/// The length of this memory map entry.
	pub len: usize,

	/// The type of this memory map entry.
	pub ty: MemmapType,
}

/// A memory map entry type.
///
/// For details, see [15. System Address Map Interfaces — ACPI Specification 6.6 documentation].
///
/// [15. System Address Map Interfaces — ACPI Specification 6.6 documentation]: https://uefi.org/specs/ACPI/6.6/15_System_Address_Map_Interfaces.html
#[derive(IntoPrimitive, TryFromPrimitive, Hash, PartialEq, Eq, Clone, Copy, Debug)]
#[non_exhaustive]
#[repr(u8)]
pub enum MemmapType {
	Ram = 1,
	Reserved = 2,
	Acpi = 3,
	Nvs = 4,
	Unusable = 5,
	Disabled = 6,
	Pmem = 7,
}
