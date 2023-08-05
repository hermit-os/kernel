use crate::executor::network::Handle;
use crate::fd::ObjectInterface;

#[derive(Debug, Clone)]
pub struct Socket(Handle);

impl Socket {
	pub fn new(handle: Handle) -> Self {
		Self(handle)
	}
}

impl ObjectInterface for Socket {}
