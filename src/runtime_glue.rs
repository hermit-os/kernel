//! Minor functions that Rust really expects to be defined by the compiler,
//! but which we need to provide manually because we're on bare metal.

use alloc::alloc::Layout;
use core::panic::PanicInfo;

use crate::arch::percore;
use crate::syscalls;

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
	let core_id = percore::core_id();
	println!("[{core_id}][PANIC] {info}");

	syscalls::shutdown(1)
}

#[alloc_error_handler]
fn rust_oom(layout: Layout) -> ! {
	let size = layout.size();
	panic!("memory allocation of {size} bytes failed")
}
