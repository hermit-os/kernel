//! Minor functions that Rust really expects to be defined by the compiler,
//! but which we need to provide manually because we're on bare metal.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
	loaderlog!("{}", info);

	loop {}
}

#[cfg(not(test))]
#[alloc_error_handler]
fn rust_oom(layout: core::alloc::Layout) -> ! {
	let size = layout.size();
	panic!("memory allocation of {size} bytes failed")
}
