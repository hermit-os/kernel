use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use async_trait::async_trait;

use crate::fd::{AddressFamily, Endpoint, IoCtl, ListenEndpoint, ObjectInterface};
use crate::io;

#[derive(Debug)]
pub(crate) struct VsockListenEndpoint {
	port: u32,
	#[allow(dead_code)]
	cid: u32,
}

impl VsockListenEndpoint {
	pub const fn new(port: u32, cid: u32) -> Self {
		Self { port, cid }
	}
}

#[derive(Debug)]
pub struct Socket {
	port: AtomicU32,
	nonblocking: AtomicBool,
	listen: AtomicBool,
}

impl Socket {
	pub fn new() -> Self {
		Self {
			port: AtomicU32::new(0),
			nonblocking: AtomicBool::new(false),
			listen: AtomicBool::new(false),
		}
	}
}

#[async_trait]
impl ObjectInterface for Socket {
	fn bind(&self, endpoint: ListenEndpoint) -> io::Result<()> {
		info!("bind {:?}", endpoint);
		match endpoint {
			ListenEndpoint::Vsock(ep) => {
				self.port.store(ep.port, Ordering::Release);
				Ok(())
			}
			#[cfg(any(feature = "tcp", feature = "udp"))]
			_ => Err(io::Error::EINVAL),
		}
	}

	fn is_nonblocking(&self) -> bool {
		self.nonblocking.load(Ordering::Acquire)
	}

	fn listen(&self, _backlog: i32) -> io::Result<()> {
		info!("listen");
		self.listen.store(true, Ordering::Relaxed);
		Ok(())
	}

	fn accept(&self) -> io::Result<Endpoint> {
		info!("accept");
		Err(io::Error::EINVAL)
	}

	fn ioctl(&self, cmd: IoCtl, value: bool) -> io::Result<()> {
		if cmd == IoCtl::NonBlocking {
			if value {
				trace!("set vsock device to nonblocking mode");
				self.nonblocking.store(true, Ordering::Release);
			} else {
				trace!("set vsock device to blocking mode");
				self.nonblocking.store(false, Ordering::Release);
			}

			Ok(())
		} else {
			Err(io::Error::EINVAL)
		}
	}

	fn get_address_family(&self) -> Option<AddressFamily> {
		Some(AddressFamily::VSOCK)
	}
}

impl Clone for Socket {
	fn clone(&self) -> Self {
		Self {
			port: AtomicU32::new(self.port.load(Ordering::Acquire)),
			nonblocking: AtomicBool::new(self.nonblocking.load(Ordering::Acquire)),
			listen: AtomicBool::new(false),
		}
	}
}
