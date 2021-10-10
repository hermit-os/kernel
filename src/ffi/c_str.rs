use core::{slice, str};

use super::c_char;

/// A port of [`std::ffi::CStr`].
///
/// # Safety
///
/// Refer to [`std::ffi::CStr`].
#[repr(transparent)]
pub struct CStr {
	inner: [c_char],
}

impl CStr {
	pub unsafe fn from_ptr<'a>(ptr: *const c_char) -> &'a Self {
		unsafe {
			let len = strlen(ptr);
			let ptr = ptr as *const u8;
			Self::from_bytes_with_nul_unchecked(slice::from_raw_parts(ptr, len as usize + 1))
		}
	}

	pub unsafe fn from_bytes_with_nul_unchecked(bytes: &[u8]) -> &Self {
		unsafe { &*(bytes as *const [u8] as *const Self) }
	}

	pub fn to_bytes(&self) -> &[u8] {
		let bytes = self.to_bytes_with_nul();
		unsafe { bytes.get_unchecked(..bytes.len() - 1) }
	}

	pub fn to_bytes_with_nul(&self) -> &[u8] {
		unsafe { &*(&self.inner as *const [c_char] as *const [u8]) }
	}

	pub fn to_str(&self) -> Result<&str, str::Utf8Error> {
		str::from_utf8(self.to_bytes())
	}
}

unsafe fn strlen(mut s: *const c_char) -> usize {
	// SAFETY: The caller must guarantee `s` points to a valid 0-terminated string.
	unsafe {
		let mut n = 0;
		while *s != 0 {
			n += 1;
			s = s.offset(1);
		}
		n
	}
}
