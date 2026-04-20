pub use self::generic::*;
pub use self::uhyve::*;
use crate::arch;

mod generic;
pub(crate) mod uhyve;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}

	fn shutdown(&self, error_code: i32) -> ! {
		// This is a stable message used for detecting exit codes for different hypervisors.
		panic_println!("exit status {error_code}");

		arch::processor::shutdown(error_code)
	}
}
