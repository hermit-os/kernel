use alloc::boxed::Box;
use alloc::vec::Vec;

pub use self::generic::*;
pub use self::uhyve::*;
use crate::{arch, env};

mod generic;
pub(crate) mod uhyve;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}

	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		let mut argv = Vec::new();

		let name = Box::leak(Box::new("{name}\0")).as_ptr();
		argv.push(name);

		let args = env::args();
		debug!("Setting argv as: {:?}", args);
		for arg in args {
			let ptr = Box::leak(format!("{arg}\0").into_boxed_str()).as_ptr();
			argv.push(ptr);
		}

		let mut envv = Vec::new();

		let envs = env::vars();
		debug!("Setting envv as: {:?}", envs);
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
		arch::processor::shutdown(error_code)
	}
}
