use alloc::boxed::Box;

use crate::fd::ObjectInterface;
use crate::net::Handle;

#[derive(Debug, Clone)]
pub struct Socket(Handle);

impl Socket {
	pub fn new(handle: Handle) -> Self {
		Self(handle)
	}
}

impl ObjectInterface for Socket {
	fn clone_box(&self) -> Box<dyn ObjectInterface> {
		Box::new(self.clone())
	}
}
