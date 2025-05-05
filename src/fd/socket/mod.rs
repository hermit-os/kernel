#[cfg(feature = "tcp")]
pub(crate) mod tcp;
#[cfg(feature = "udp")]
pub(crate) mod udp;
#[cfg(feature = "virtio-vsock")]
pub(crate) mod vsock;

#[macro_export]
macro_rules! socket_handle_ioctl {
	($this: ident, $cmd: ident, $argp: ident) => {{
		use crate::errno::Errno;
		use crate::executor::block_on;
		use crate::fd;

		const FIONBIO: u32 = 0x8008_667eu32;

		if $cmd.into_bits() == FIONBIO {
			let value = unsafe { *($argp as *const i32) };
			let status_flags = if value != 0 {
				fd::StatusFlags::O_NONBLOCK
			} else {
				fd::StatusFlags::empty()
			};

			block_on($this.set_status_flags(status_flags), None)
		} else {
			Err(Errno::Inval)
		}
	}};
}
