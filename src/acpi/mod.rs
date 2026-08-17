#![cfg_attr(
	any(target_arch = "aarch64", target_arch = "riscv64"),
	expect(dead_code)
)]

mod handler;

use core::num::NonZero;

use acpi::platform::AcpiPlatform;
use acpi::{AcpiTable, AcpiTables, Handler, PhysicalMapping, aml};
use hermit_sync::OnceCell;

use self::handler::AcpiHandler;
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

#[expect(dead_code)]
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
