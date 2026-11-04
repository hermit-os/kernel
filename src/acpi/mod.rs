#![cfg_attr(
	any(target_arch = "aarch64", target_arch = "riscv64"),
	expect(dead_code, unused_imports)
)]

mod handler;
mod shutdown;
mod spec;

#[cfg(all(target_arch = "x86_64", not(feature = "pci"), feature = "virtio"))]
use alloc::vec::Vec;
use core::num::NonZero;
#[cfg(all(target_arch = "x86_64", not(feature = "pci"), feature = "virtio"))]
use core::str::FromStr;

#[cfg(all(target_arch = "x86_64", not(feature = "pci"), feature = "virtio"))]
use acpi::aml::{
	Interpreter,
	namespace::{AmlName, NamespaceLevelKind},
	object::Object,
	resource::{Resource, resource_descriptor_list},
};
use acpi::platform::AcpiPlatform;
use acpi::{AcpiTable, AcpiTables, Handler, PhysicalMapping};
use hermit_sync::OnceCell;

use self::handler::AcpiHandler;
pub use self::shutdown::*;
pub use self::spec::*;
use crate::env::{self, StartInfo};

static ACPI_PLATFORM: OnceCell<AcpiPlatform<AcpiHandler>> = OnceCell::new();

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
	ACPI_PLATFORM
		.set(platform)
		.unwrap_or_else(|_| panic!("ACPI platform should not be initialized"));
}

pub fn find_table<T: AcpiTable>() -> Option<PhysicalMapping<AcpiHandler, T>> {
	ACPI_PLATFORM.get()?.tables.find_table()
}

/// Returns the `_CRS` resources of each device with the hardware ID `hid` in the DSDT and SSDTs.
#[cfg(all(target_arch = "x86_64", not(feature = "pci"), feature = "virtio"))]
pub fn find_devices(hid: &str) -> Vec<Vec<Resource>> {
	let Some(platform) = ACPI_PLATFORM.get() else {
		return Vec::new();
	};
	let interpreter = match Interpreter::new_from_platform(platform) {
		Ok(interpreter) => interpreter,
		Err(err) => {
			warn!("Could not load the AML tables: {err:?}");
			return Vec::new();
		}
	};

	let mut devices = Vec::new();
	let traversal = interpreter.namespace.lock().traverse(|path, level| {
		if level.kind == NamespaceLevelKind::Device {
			devices.push(path.clone());
		}
		Ok(true)
	});
	if let Err(err) = traversal {
		warn!("Could not walk the ACPI namespace: {err:?}");
	}

	let evaluate = |device: &AmlName, name: &str| {
		let path = AmlName::from_str(name).unwrap().resolve(device).ok()?;
		interpreter.evaluate(path, Vec::new()).ok()
	};
	devices
		.iter()
		.filter(|device| {
			evaluate(device, "_HID")
				.is_some_and(|id| matches!(&*id, Object::String(id) if id == hid))
		})
		.filter_map(|device| resource_descriptor_list(evaluate(device, "_CRS")?).ok())
		.collect()
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
