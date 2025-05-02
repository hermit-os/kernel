use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr;

pub use self::generic::*;
pub use self::uhyve::*;
use crate::{arch, env};

#[linkage = "weak"]
#[no_mangle]

static __hermit_app_name: *const u8 = ptr::null();

mod generic;
pub(crate) mod uhyve;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}

	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		let mut argv = Vec::new();

		let name = unsafe {
			if !__hermit_app_name.is_null() {
				__hermit_app_name
			} else {
				Box::leak(Box::new("hermit-app\0")).as_ptr() // default binary name
			}
		};

		argv.push(name);

		let args = env::args();
		debug!("Setting argv as: {args:?}");
		for arg in args {
			let ptr = Box::leak(format!("{arg}\0").into_boxed_str()).as_ptr();
			argv.push(ptr);
		}

		let mut envv = Vec::new();

		let envs = env::vars();
		debug!("Setting envv as: {envs:?}");
		for (key, value) in envs {
			let ptr = Box::leak(format!("{key}={value}\0").into_boxed_str()).as_ptr();
			envv.push(ptr);
		}
		envv.push(core::ptr::null::<u8>());

		let argc = argv.len() as i32;
		let argv = argv.leak().as_ptr();
		// do we have more than a end marker? If not, return as null pointer
		let envv = if envv.len() == 1 {
			core::ptr::null::<*const u8>()
		} else {
			envv.leak().as_ptr()
		};

		(argc, argv, envv)
	}

	fn shutdown(&self, error_code: i32) -> ! {
		// This is a stable message used for detecting exit codes for different hypervisors.
		panic_println!("exit status {error_code}");

		arch::processor::shutdown(error_code)
	}
}
