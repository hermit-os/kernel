#![cfg_attr(not(feature = "tcp"), expect(dead_code))]

#[cfg(feature = "tcp")]
pub(crate) mod tcp;
#[cfg(feature = "udp")]
pub(crate) mod udp;
#[cfg(feature = "virtio-vsock")]
pub(crate) mod vsock;

/// Further receives will be disallowed
pub const SHUT_RD: i32 = 0;
/// Further sends will be disallowed
pub const SHUT_WR: i32 = 1;
/// Further sends and receives will be disallowed
pub const SHUT_RDWR: i32 = 2;

/// The default queue size for incoming connections
pub const DEFAULT_BACKLOG: i32 = 128;

/// The maximum queue size for incoming connections,
/// based on the default maximum used by modern Linux.
pub const SOMAXCONN: i32 = 4096;

#[macro_export]
macro_rules! socket_handle_ioctl {
	($this: ident, $cmd: ident, $argp: ident) => {{
		use $crate::errno::Errno;
		use $crate::fd::{IoCtlCall, StatusFlags};

		const FIONBIO: IoCtlCall = IoCtlCall::from_bits(0x8008_667eu32);

		if $cmd == FIONBIO {
			let value = unsafe { *($argp as *const i32) };
			let status_flags = if value != 0 {
				StatusFlags::O_NONBLOCK
			} else {
				StatusFlags::empty()
			};

			$this.set_status_flags(status_flags).await
		} else {
			Err(Errno::Inval)
		}
	}};
}
