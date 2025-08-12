use crate::errno::Errno;
use crate::executor::block_on;
use crate::fd::ObjectInterface;
use crate::{fd, io};

#[cfg(feature = "tcp")]
pub(crate) mod tcp;
#[cfg(feature = "udp")]
pub(crate) mod udp;
#[cfg(feature = "vsock")]
pub(crate) mod vsock;

/// Handles an ioctl (general function)
fn socket_handle_ioctl(
	this: &dyn ObjectInterface,
	cmd: crate::fs::ioctl::IoCtlCall,
	argp: *mut core::ffi::c_void,
) -> io::Result<()> {
	const FIONBIO: u32 = 0x8008_667eu32;

	if cmd.into_bits() == FIONBIO {
		let value = unsafe { *(argp as *const i32) };
		let status_flags = if value != 0 {
			fd::StatusFlags::O_NONBLOCK
		} else {
			fd::StatusFlags::empty()
		};

		block_on(this.set_status_flags(status_flags), None)
	} else {
		Err(Errno::Inval)
	}
}
