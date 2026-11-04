use alloc::vec::Vec;
use core::str::FromStr;

use acpi::aml::Interpreter;
use acpi::aml::namespace::{AmlName, NamespaceLevelKind};
use acpi::aml::object::Object;
use acpi::aml::resource::{Resource, resource_descriptor_list};

use super::ACPI_PLATFORM;

/// Returns the `_CRS` resources of each device with the hardware ID `hid` in the DSDT and SSDTs,
/// or `None` if there are no ACPI tables.
pub fn find_devices(hid: &str) -> Option<Vec<Vec<Resource>>> {
	let platform = ACPI_PLATFORM.get()?;
	let interpreter = match Interpreter::new_from_platform(platform) {
		Ok(interpreter) => interpreter,
		Err(err) => {
			warn!("Could not load the AML tables: {err:?}");
			return Some(Vec::new());
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
	let resources = devices
		.iter()
		.filter(|device| {
			evaluate(device, "_HID")
				.is_some_and(|id| matches!(&*id, Object::String(id) if id == hid))
		})
		.filter_map(|device| resource_descriptor_list(evaluate(device, "_CRS")?).ok())
		.collect();
	Some(resources)
}
