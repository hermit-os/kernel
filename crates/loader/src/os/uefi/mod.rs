mod console;

use alloc::string::String;
use alloc::vec::Vec;

use log::info;
use qemu_exit::QEMUExit;
use uefi::fs::{FileSystem, Path};
use uefi::prelude::*;

pub use self::console::CONSOLE;

// Entry Point of the Uefi Loader
#[entry]
fn loader_main(_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
	uefi::helpers::init(&mut system_table).unwrap();
	crate::log::init();

	let app = read_app(system_table.boot_services());

	let string = String::from_utf8(app).unwrap();
	println!("{string}");

	let custom_exit_success = 3;
	let qemu_exit_handle = qemu_exit::X86::new(0xf4, custom_exit_success);
	qemu_exit_handle.exit_success()
}

fn read_app(bt: &BootServices) -> Vec<u8> {
	let fs = bt
		.get_image_file_system(bt.image_handle())
		.expect("should open file system");

	let path = Path::new(cstr16!(r"\efi\boot\hermit-app"));

	let data = FileSystem::new(fs)
		.read(path)
		.expect("should read file content");

	let len = data.len();
	info!("Read Hermit application from \"{path}\" (size = {len} B)");

	data
}
