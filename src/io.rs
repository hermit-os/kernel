use embedded_io::{Error, ErrorKind};

use crate::errno::Errno;

pub type Result<T> = core::result::Result<T, Errno>;

impl From<Errno> for ErrorKind {
	fn from(value: Errno) -> Self {
		match value {
			Errno::Noent => ErrorKind::NotFound,
			Errno::Acces | Errno::Perm => ErrorKind::PermissionDenied,
			Errno::Connrefused => ErrorKind::ConnectionRefused,
			Errno::Connreset => ErrorKind::ConnectionReset,
			Errno::Connaborted => ErrorKind::ConnectionAborted,
			Errno::Notconn => ErrorKind::NotConnected,
			Errno::Addrinuse => ErrorKind::AddrInUse,
			Errno::Addrnotavail => ErrorKind::AddrNotAvailable,
			Errno::Pipe => ErrorKind::BrokenPipe,
			Errno::Exist => ErrorKind::AlreadyExists,
			Errno::Inval => ErrorKind::InvalidInput,
			Errno::Timedout => ErrorKind::TimedOut,
			Errno::Intr => ErrorKind::Interrupted,
			Errno::Opnotsupp => ErrorKind::Unsupported,
			Errno::Nomem => ErrorKind::OutOfMemory,
			_ => ErrorKind::Other,
		}
	}
}

impl Error for Errno {
	fn kind(&self) -> ErrorKind {
		(*self).into()
	}
}
