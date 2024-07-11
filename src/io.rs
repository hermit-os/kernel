use alloc::collections::{TryReserveError, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::{cmp, fmt, result};
use core::str;
use enum_dispatch::enum_dispatch;

// TODO: Integrate with src/errno.rs ?
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq, FromPrimitive, ToPrimitive)]
pub enum Error {
	ENOENT = crate::errno::ENOENT as isize,
	ENOSYS = crate::errno::ENOSYS as isize,
	EIO = crate::errno::EIO as isize,
	EBADF = crate::errno::EBADF as isize,
	EISDIR = crate::errno::EISDIR as isize,
	EINVAL = crate::errno::EINVAL as isize,
	ETIME = crate::errno::ETIME as isize,
	EAGAIN = crate::errno::EAGAIN as isize,
	EFAULT = crate::errno::EFAULT as isize,
	ENOBUFS = crate::errno::ENOBUFS as isize,
	ENOTCONN = crate::errno::ENOTCONN as isize,
	ENOTDIR = crate::errno::ENOTDIR as isize,
	EMFILE = crate::errno::EMFILE as isize,
	EEXIST = crate::errno::EEXIST as isize,
	EADDRINUSE = crate::errno::EADDRINUSE as isize,
	EOVERFLOW = crate::errno::EOVERFLOW as isize,
	ENOMEM = crate::errno::ENOMEM as isize,
}

impl From<TryReserveError> for Error {
	fn from(_value: TryReserveError) -> Self {
		Self::ENOMEM
	}
}

pub type Result<T> = result::Result<T, Error>;

/// The Read trait allows for reading bytes from a source.
///
/// The Read trait is derived from Rust's std library.
#[enum_dispatch]
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
#[enum_dispatch]
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

impl Read for &[u8] {
	#[inline]
	fn read(&mut self,buf: &mut [u8]) -> Result<usize> {
		let amt = cmp::min(buf.len(), self.len());
        let (a, b) = self.split_at(amt);

        // First check if the amount of bytes we want to read is small:
        // `copy_from_slice` will generally expand to a call to `memcpy`, and
        // for a single byte the overhead is significant.
        if amt == 1 {
            buf[0] = a[0];
        } else {
            buf[..amt].copy_from_slice(a);
        }

        *self = b;
        Ok(amt)
	}

	#[inline]
	fn read_to_end(&mut self,buf: &mut Vec<u8>) -> Result<usize> {
        let len = self.len();
        buf.try_reserve(len)?;
        buf.extend_from_slice(self);
        *self = &self[len..];
        Ok(len)
	}

	#[inline]
	fn read_to_string(&mut self,buf: &mut String) -> Result<usize> {
        let content = str::from_utf8(self).map_err(|_| Error::EINVAL)?;
        let len = self.len();
        buf.try_reserve(len)?;
        buf.push_str(content);
        *self = &self[len..];
        Ok(len)
	}
}

impl Read for VecDeque<u8> {
	#[inline]
	fn read(&mut self,buf: &mut [u8]) -> Result<usize> {
		let (ref mut front, _) = self.as_slices();
        let n = Read::read(front, buf)?;
        self.drain(..n);
        Ok(n)
	}

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        // The total len is known upfront so we can reserve it in a single call.
        let len = self.len();
        buf.try_reserve(len)?;

        let (front, back) = self.as_slices();
        buf.extend_from_slice(front);
        buf.extend_from_slice(back);
        self.clear();
        Ok(len)
    }

    #[inline]
    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        // SAFETY: We only append to the buffer
        unsafe { append_to_string(buf, |buf| self.read_to_end(buf)) }
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn append_to_string<F>(buf: &mut String, f: F) -> Result<usize>
where
    F: FnOnce(&mut Vec<u8>) -> Result<usize>,
{
	struct Guard<'a> {
		buf: &'a mut Vec<u8>,
		len: usize,
	}
	
	impl Drop for Guard<'_> {
		fn drop(&mut self) {
			unsafe {
				self.buf.set_len(self.len);
			}
		}
	}

    let mut g = Guard { len: buf.len(), buf: buf.as_mut_vec() };
    let ret = f(g.buf);

    // SAFETY: the caller promises to only append data to `buf`
    let appended = g.buf.get_unchecked(g.len..);
    if str::from_utf8(appended).is_err() {
        ret.and_then(|_| Err(Error::EINVAL))
    } else {
        g.len = g.buf.len();
        ret
    }
}
