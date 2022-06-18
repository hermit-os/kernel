use uefi::prelude::*;

// Entry Point of the Uefi Loader
#[entry]
fn loader_main(_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
	uefi_services::init(&mut system_table).unwrap();

	Status::SUCCESS
}
