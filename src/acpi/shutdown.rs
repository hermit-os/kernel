use core::slice;

use acpi::address::MappedGas;
use acpi::registers::FixedRegisters;
use acpi::sdt::SdtHeader;
use acpi::sdt::fadt::Fadt;
use acpi::{AcpiError, AmlTable, Handler};
use bit_field::BitField;

use super::ACPI_PLATFORM;

/// Enters the ACPI S5 soft off system state.
///
/// For details, see [Transitioning from the Working to the Soft Off State].
///
/// [Transitioning from the Working to the Soft Off State]: https://uefi.org/specs/ACPI/6.6/16_Waking_and_Sleeping.html#transitioning-from-the-working-to-the-soft-off-state
pub fn shutdown() -> Option<!> {
	debug!("Entering ACPI S5 soft off state...");

	let slp_typa = slp_typa().unwrap();

	let fadt = super::find_table::<Fadt>()?;

	let fadt_flags = fadt.flags;
	if fadt_flags.system_is_hw_reduced_acpi() {
		debug!("HW-reduced ACPI platform.");

		write_sleep_control_reg(&fadt, slp_typa, &fadt.handler).ok()?;
	} else {
		debug!("Not a HW-reduced ACPI platform.");

		let fixed_registers = FixedRegisters::new(&fadt, fadt.handler.clone()).ok()?;
		write_pm1x_cnt(&fixed_registers.pm1_control_registers.pm1a, slp_typa).ok()?;
	}

	None
}

fn slp_typa() -> Option<u8> {
	let acpi_platform = ACPI_PLATFORM.get().unwrap();

	if let Ok(dsdt) = acpi_platform.tables.dsdt()
		&& let Some(slp_typa) = find_slp_typa(&dsdt, &acpi_platform.handler)
	{
		return Some(slp_typa);
	}

	for ssdt in acpi_platform.tables.ssdts() {
		if let Some(slp_typa) = find_slp_typa(&ssdt, &acpi_platform.handler) {
			return Some(slp_typa);
		}
	}

	None
}

/// Writes the provided SLP_TYPx with the SLP_ENx bit set into the provided PM1A_CNT register.
///
/// For details, see [PM1 Control Registers].
///
/// [PM1 Control Registers]: https://uefi.org/specs/ACPI/6.6/04_ACPI_Hardware_Specification.html#pm1-control-registers-2
fn write_pm1x_cnt<H: Handler>(pm1x_cnt: &MappedGas<H>, slp_typx: u8) -> Result<(), AcpiError> {
	let mut value = pm1x_cnt.read()?;
	// SLP_TYPx
	value.set_bits(10..13, u64::from(slp_typx));
	// SLP_EN
	value.set_bit(13, true);
	pm1x_cnt.write(value)?;

	Ok(())
}

/// Writes the provided HW-reduced ACPI Sleep Type value and the SLP_EN bit to the Sleep Control Register.
///
/// For details, see [Sleep Control and Status Registers].
///
/// [Sleep Control and Status Registers]: https://uefi.org/specs/ACPI/6.6/04_ACPI_Hardware_Specification.html#sleep-control-and-status-registers
fn write_sleep_control_reg<H: Handler>(
	fadt: &Fadt,
	slp_typx: u8,
	handler: &H,
) -> Result<(), AcpiError> {
	let sleep_control_reg = fadt.sleep_control_register()?;
	let sleep_control_reg = sleep_control_reg.ok_or(AcpiError::InvalidGenericAddress)?;
	let sleep_control_reg = unsafe { MappedGas::map_gas(sleep_control_reg, handler)? };

	let mut value = sleep_control_reg.read()?;
	// SLP_TYPx
	value.set_bits(2..5, u64::from(slp_typx));
	// SLP_EN
	value.set_bit(5, true);
	sleep_control_reg.write(value)?;

	Ok(())
}

/// ACPI AML opcode indicating that a name follows.
const AML_NAMEOP: u8 = 0x08;
/// ACPI AML opcode indicating that a package follows.
const AML_PACKAGEOP: u8 = 0x12;
/// ACPI AML opcode indicating a single zero byte as the data.
const AML_ZEROOP: u8 = 0x00;
/// ACPI AML opcode indicating a single one byte as the data.
const AML_ONEOP: u8 = 0x01;
/// ACPI AML opcode indicating that a single byte with the data follows.
const AML_BYTEPREFIX: u8 = 0x0a;

/// Searches for SLP_TYPa in the provided AML table.
///
/// Note that we do not have a proper AML interpreter, which would be slower and be too large for our kernel stack without optimizations.
fn find_slp_typa<H: Handler>(aml_table: &AmlTable, handler: &H) -> Option<u8> {
	let mapping = unsafe {
		handler.map_physical_region::<SdtHeader>(aml_table.phys_address, aml_table.length as usize)
	};
	let ptr = unsafe {
		mapping
			.virtual_start
			.as_ptr()
			.cast::<u8>()
			.add(size_of::<SdtHeader>())
	};
	let len = aml_table.length as usize - size_of::<SdtHeader>();
	let stream = unsafe { slice::from_raw_parts(ptr, len) };

	// Find the "_S5_" object in the bytecode.
	let s5 = [b'_', b'S', b'5', b'_', AML_PACKAGEOP];
	let s5_position = stream.windows(s5.len()).position(|window| window == s5);
	let i = s5_position?;

	// We have found an "_S5_" object that looks valid.
	// To be sure, verify that it begins with an AML_NAMEOP or an AML_NAMEOP and a backslash.
	if i > 2
		&& (stream[i - 1] == AML_NAMEOP || (stream[i - 2] == AML_NAMEOP && stream[i - 1] == b'\\'))
	{
		// This is a valid "_S5_" object.
		// It should be followed by this structure:
		//    - single byte for PkgLength (index 5)
		//    - single byte for NumElements (index 6)
		let pkg_length = stream[i + 5];
		let num_elements = stream[i + 6];

		// Bits 6-7 of PkgLength are non-zero for larger packages, resulting in a different structure.
		// This mustn't be the case for the "_S5_" object.
		if pkg_length & 0b1100_0000 == 0 && num_elements > 0 {
			// The next byte is an opcode describing the data.
			// It is usually the byte prefix, indicating that the actual data is the single byte following the opcode.
			// However, if the data is a zero or one byte, this may also be indicated by the opcode.
			let op = stream[i + 7];
			let slp_typa = match op {
				AML_ZEROOP => 0,
				AML_ONEOP => 1,
				AML_BYTEPREFIX => stream[i + 8],
				_ => return None,
			};

			// All assumptions are correct, so slp_typa is supposed to contain valid information.
			// Now we have all information we need for powering off through ACPI.
			//
			// Note that Power Off may also be controlled through PM1B_CNT_BLK / SLP_TYPB
			// according to the ACPI Specification. However, this has not yet been observed on real computers
			// and therefore not implemented.
			return Some(slp_typa);
		}
	}

	None
}
