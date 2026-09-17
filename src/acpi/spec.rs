bitflags! {
	/// Multiple APIC Flags.
	///
	/// For reference, see [Table 5.20 Multiple APIC Flags — 5. ACPI Software Programming Model — ACPI Specification 6.6 documentation].
	///
	/// [Table 5.20 Multiple APIC Flags — 5. ACPI Software Programming Model — ACPI Specification 6.6 documentation]: https://uefi.org/specs/ACPI/6.6/05_ACPI_Software_Programming_Model.html#multiple-apic-flags
	pub struct MultipleApicFlags: u32 {
		/// A one indicates that the system also has a PC-AT-compatible dual-8259 setup.
		///
		/// The 8259 vectors must be disabled (that is, masked) when enabling the ACPI APIC operation.
		const PCAT_COMPAT = 1;
	}
}
