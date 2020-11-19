// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Minor functions that Rust really expects to be defined by the compiler,
//! but which we need to provide manually because we're on bare metal.

use crate::arch::kernel::processor::run_on_hypervisor;
use crate::{__sys_shutdown, arch};
use alloc::alloc::Layout;
use core::panic::PanicInfo;

// see https://users.rust-lang.org/t/psa-breaking-change-panic-fmt-language-item-removed-in-favor-of-panic-implementation/17875
#[cfg(target_os = "hermit")]
#[linkage = "weak"]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	print!("[{}][!!!PANIC!!!] ", arch::percore::core_id());

	if let Some(location) = info.location() {
		print!("{}:{}: ", location.file(), location.line());
	}

	if let Some(message) = info.message() {
		print!("{}", message);
	}

	println!();

	if run_on_hypervisor() {
		__sys_shutdown(1);
	}

	loop {
		arch::processor::halt();
	}
}

#[linkage = "weak"]
#[alloc_error_handler]
fn rust_oom(layout: Layout) -> ! {
	println!(
		"[{}][!!!OOM!!!] Memory allocation of {} bytes failed",
		arch::percore::core_id(),
		layout.size()
	);

	loop {
		arch::processor::halt();
	}
}

#[no_mangle]
pub unsafe extern "C" fn __rg_oom(size: usize, align: usize) -> ! {
	let layout = Layout::from_size_align_unchecked(size, align);
	rust_oom(layout)
}
