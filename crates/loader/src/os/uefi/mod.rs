mod allocator;
mod console;

use alloc::string::String;
use alloc::vec::Vec;

use log::info;
use qemu_exit::QEMUExit;
use uefi::fs::{FileSystem, Path};
use uefi::prelude::*;
use uefi::table::boot::MemoryType;

pub use self::console::CONSOLE;

// Entry Point of the Uefi Loader
#[entry]
fn main() -> Status {
	uefi::helpers::init().unwrap();
	crate::log::init();

	let app = read_app();

	let string = String::from_utf8(app).unwrap();
	println!("{string}");

	allocator::exit_boot_services();
	let _memory_map = unsafe { boot::exit_boot_services(MemoryType::LOADER_DATA) };

	println!("Exited boot services!");
	println!("Allocations still {}!", String::from("work"));

	let custom_exit_success = 3;
	let qemu_exit_handle = qemu_exit::X86::new(0xf4, custom_exit_success);
	qemu_exit_handle.exit_success()
}

fn read_app() -> Vec<u8> {
	let image_handle = boot::image_handle();
	let fs = boot::get_image_file_system(image_handle).expect("should open file system");

	let path = Path::new(cstr16!(r"\efi\boot\hermit-app"));

	let data = FileSystem::new(fs)
		.read(path)
		.expect("should read file content");

	let len = data.len();
	info!("Read Hermit application from \"{path}\" (size = {len} B)");

	data
}
