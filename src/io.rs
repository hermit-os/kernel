use alloc::string::String;
use alloc::vec::Vec;
use core::{fmt, result};

use num_enum::{IntoPrimitive, TryFromPrimitive};

// TODO: Integrate with src/errno.rs ?
#[allow(clippy::upper_case_acronyms)]
#[derive(TryFromPrimitive, IntoPrimitive, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(i32)]
pub enum Error {
	ENOENT = crate::errno::ENOENT,
	ENOSYS = crate::errno::ENOSYS,
	EIO = crate::errno::EIO,
	EBADF = crate::errno::EBADF,
	EISDIR = crate::errno::EISDIR,
	EINVAL = crate::errno::EINVAL,
	ETIME = crate::errno::ETIME,
	EAGAIN = crate::errno::EAGAIN,
	EFAULT = crate::errno::EFAULT,
	ENOBUFS = crate::errno::ENOBUFS,
	ENOTCONN = crate::errno::ENOTCONN,
	ENOTDIR = crate::errno::ENOTDIR,
	EMFILE = crate::errno::EMFILE,
	EEXIST = crate::errno::EEXIST,
	EADDRINUSE = crate::errno::EADDRINUSE,
	EOVERFLOW = crate::errno::EOVERFLOW,
	ENOTSOCK = crate::errno::ENOTSOCK,
}

pub type Result<T> = result::Result<T, Error>;

/// The Read trait allows for reading bytes from a source.
///
/// The Read trait is derived from Rust's std library.
pub trait Read {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

	/// Read all bytes until EOF in this source, placing them into buf.
	fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
		let start_len = buf.len();

		loop {
			let mut probe = [0u8; 512];

			match self.read(&mut probe) {
				Ok(0) => return Ok(buf.len() - start_len),
				Ok(n) => {
					buf.extend_from_slice(&probe[..n]);
				}
				Err(e) => return Err(e),
			}
		}
	}

	/// Read all bytes until EOF in this source, appending them to `buf`.
	///
	/// If successful, this function returns the number of bytes which were read
	/// and appended to `buf`.
	fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
		unsafe { self.read_to_end(buf.as_mut_vec()) }
	}
}

/// The Write trait allows for reading bytes from a source.
///
/// The Write trait is derived from Rust's std library.
pub trait Write {
	fn write(&mut self, buf: &[u8]) -> Result<usize>;

	/// Attempts to write an entire buffer into this writer.
	fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
		while !buf.is_empty() {
			match self.write(buf) {
				Ok(0) => {
					return Err(Error::EIO);
				}
				Ok(n) => buf = &buf[n..],
				Err(e) => return Err(e),
			}
		}

		Ok(())
	}

	/// Writes a formatted string into this writer, returning any error encountered.
	fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> Result<()> {
		// Create a shim which translates a Write to a fmt::Write and saves
		// off I/O errors. instead of discarding them
		struct Adapter<'a, T: ?Sized> {
			inner: &'a mut T,
			error: Result<()>,
		}

		impl<T: Write + ?Sized> fmt::Write for Adapter<'_, T> {
			fn write_str(&mut self, s: &str) -> fmt::Result {
				match self.inner.write_all(s.as_bytes()) {
					Ok(()) => Ok(()),
					Err(e) => {
						self.error = Err(e);
						Err(fmt::Error)
					}
				}
			}
		}

		let mut output = Adapter {
			inner: self,
			error: Ok(()),
		};
		match fmt::write(&mut output, fmt) {
			Ok(()) => Ok(()),
			Err(..) => {
				// check if the error came from the underlying `Write` or not
				if output.error.is_err() {
					output.error
				} else {
					Err(Error::EINVAL)
				}
			}
		}
	}
}
