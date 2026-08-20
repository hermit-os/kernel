#![cfg_attr(
	any(target_arch = "aarch64", target_arch = "riscv64"),
	expect(dead_code, unused_imports)
)]

mod handler;
mod spec;

use alloc::vec::Vec;
use core::num::NonZero;
use core::str::FromStr;

use acpi::address::MappedGas;
use acpi::aml::namespace::AmlName;
use acpi::aml::object::{Object, WrappedObject};
use acpi::platform::AcpiPlatform;
use acpi::registers::FixedRegisters;
use acpi::sdt::fadt::Fadt;
use acpi::{AcpiError, AcpiTable, AcpiTables, Handler, PhysicalMapping, aml};
use bit_field::BitField;
use hermit_sync::OnceCell;

use self::handler::AcpiHandler;
pub use self::spec::*;
use crate::env::{self, StartInfo};

static ACPI_PLATFORM: OnceCell<AcpiPlatform<AcpiHandler>> = OnceCell::new();
static AML_INTERPRETER: OnceCell<aml::Interpreter<AcpiHandler>> = OnceCell::new();

pub fn init() {
	#[cfg(feature = "uhyve")]
	use env::UhyveStartInfo;

	#[cfg(feature = "uhyve")]
	if env::start_info().is_uhyve() {
		return;
	}

	let handler = AcpiHandler::default();

	let Some(rsdp_paddr) = rsdp_paddr(&handler) else {
		return;
	};

	info!("Reading ACPI tables...");
	let tables = unsafe { AcpiTables::from_rsdp(handler.clone(), rsdp_paddr.get()).unwrap() };
	let platform = AcpiPlatform::new(tables, handler).unwrap();
	let platform = ACPI_PLATFORM
		.try_insert(platform)
		.unwrap_or_else(|_| panic!("ACPI platform should not be initialized"));

	info!("Creating AML interpreter...");
	let aml_interpreter = aml::Interpreter::new_from_platform(platform).unwrap();
	AML_INTERPRETER
		.set(aml_interpreter)
		.unwrap_or_else(|_| panic!("AML interpreter should not be initialized"));
}

pub fn find_table<T: AcpiTable>() -> Option<PhysicalMapping<AcpiHandler, T>> {
	ACPI_PLATFORM.get()?.tables.find_table()
}

#[cfg_attr(not(target_arch = "x86_64"), expect(unused_variables))]
fn rsdp_paddr<H: Handler>(handler: &H) -> Option<NonZero<usize>> {
	if let Some(rsdp_paddr) = env::start_info().rsdp_addr() {
		info!("Found RSDP paddr in start info: {rsdp_paddr:#x}");
		return Some(rsdp_paddr);
	}

	#[cfg(target_arch = "x86_64")]
	if let Ok(rsdp) = unsafe { acpi::rsdp::Rsdp::search_for_on_bios(handler.clone()) } {
		let rsdp_paddr = rsdp.virtual_start.addr();
		info!("Found RSDP paddr by searching on BIOS systems: {rsdp_paddr:#x}");
		return Some(rsdp_paddr);
	}

	warn!("Could not find RSDP paddr.");
	None
}

/// Enters the ACPI S5 soft off system state.
///
/// For details, see [Transitioning from the Working to the Soft Off State].
///
/// [Transitioning from the Working to the Soft Off State]: https://uefi.org/specs/ACPI/6.6/16_Waking_and_Sleeping.html#transitioning-from-the-working-to-the-soft-off-state
pub fn shutdown() -> Option<!> {
	debug!("Entering ACPI S5 soft off state...");

	let aml_interpreter = AML_INTERPRETER.get()?;

	// Execute the \_PTS (Prepare To Sleep) control method if available. It is defined in
	// https://uefi.org/specs/ACPI/6.6/07_Power_and_Performance_Mgmt.html#pts-prepare-to-sleep
	let pts_path = AmlName::from_str(r"\_PTS").ok()?;
	let pts_args = vec![Object::Integer(5).wrap()];
	if let Err(err) = aml_interpreter.evaluate(pts_path, pts_args) {
		debug!("Could not execute \\_PTS: {err:?}");
	}

	// Read the S5 system state package. The contents are defined in
	// https://uefi.org/specs/ACPI/6.6/07_Power_and_Performance_Mgmt.html#sx-system-states
	let s5_path = AmlName::from_str(r"\_S5").ok()?;
	let s5_object = aml_interpreter.evaluate(s5_path, Vec::new()).ok()?;
	let Object::Package(s5) = &*s5_object else {
		return None;
	};

	let fadt = find_table::<Fadt>()?;

	let fadt_flags = fadt.flags;
	if fadt_flags.system_is_hw_reduced_acpi() {
		debug!("HW-reduced ACPI platform.");

		write_sleep_control_reg(&fadt, &s5[0], &fadt.handler).ok()?;
	} else {
		debug!("Not a HW-reduced ACPI platform.");

		let fixed_registers = FixedRegisters::new(&fadt, fadt.handler.clone()).ok()?;

		write_pm1x_cnt(&fixed_registers.pm1_control_registers.pm1a, &s5[0]).ok()?;

		if let Some(pm1b_cnt) = &fixed_registers.pm1_control_registers.pm1b {
			write_pm1x_cnt(pm1b_cnt, &s5[1]).ok()?;
		}
	}

	None
}

/// Writes the provided SLP_TYPx with the SLP_ENx bit set into the provided PM1A_CNT register.
///
/// For details, see [PM1 Control Registers].
///
/// [PM1 Control Registers]: https://uefi.org/specs/ACPI/6.6/04_ACPI_Hardware_Specification.html#pm1-control-registers-2
fn write_pm1x_cnt<H: Handler>(
	pm1x_cnt: &MappedGas<H>,
	slp_typx: &WrappedObject,
) -> Result<(), AcpiError> {
	let slp_typx = slp_typx.as_integer().map_err(AcpiError::Aml)?;

	let mut value = pm1x_cnt.read()?;
	// SLP_TYPx
	value.set_bits(10..13, slp_typx);
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
	slp_typx: &WrappedObject,
	handler: &H,
) -> Result<(), AcpiError> {
	let slp_typx = slp_typx.as_integer().map_err(AcpiError::Aml)?;

	let sleep_control_reg = fadt.sleep_control_register()?;
	let sleep_control_reg = sleep_control_reg.ok_or(AcpiError::InvalidGenericAddress)?;
	let sleep_control_reg = unsafe { MappedGas::map_gas(sleep_control_reg, handler)? };

	let mut value = sleep_control_reg.read()?;
	// SLP_TYPx
	value.set_bits(2..5, slp_typx);
	// SLP_EN
	value.set_bit(5, true);
	sleep_control_reg.write(value)?;

	Ok(())
}
