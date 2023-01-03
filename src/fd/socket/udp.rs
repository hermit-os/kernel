use crate::fd::ObjectInterface;
use crate::net::Handle;

#[derive(Debug, Clone)]
pub struct Socket(Handle);

impl Socket {
	pub fn new(handle: Handle) -> Self {
		Self(handle)
	}
}

impl ObjectInterface for Socket {}
