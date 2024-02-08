use qemu_exit::QEMUExit;
use uefi::prelude::*;

// Entry Point of the Uefi Loader
#[entry]
fn loader_main(_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
	uefi_services::init(&mut system_table).unwrap();

	log::info!("Hello, UEFI!");

	let custom_exit_success = 3;
	let qemu_exit_handle = qemu_exit::X86::new(0xf4, custom_exit_success);
	qemu_exit_handle.exit_success()
}
