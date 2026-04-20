pub use self::generic::*;
pub use self::uhyve::*;

mod generic;
pub(crate) mod uhyve;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}
}
