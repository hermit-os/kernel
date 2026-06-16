use core::mem::MaybeUninit;

/// Initializes a buffer.
///
/// This function initializes the provided buffer with arbitrary data. This is
/// useful for passing user-allocated maybe-uninitialized memory to
/// `Read::read` or similar functions.
///
/// No guarantee is being made about the values that the buffer is initialized
/// with.
///
/// # Current implementation
///
/// This currently performs no actual instructions to initialize the buffer.
/// Instead, it provides an assembly story that initializes the buffer with
/// arbitrary values. This might change in the future, if Hermit gains support
/// for memory allocation techniques. Those memory allocation techniques could
/// make multiple reads from memory that has not been written to return
/// different values. In that case, appropriate steps have to be taken here.
///
/// An example of such a technique on Linux is `MADV_FREE`, which would require
/// writing at least once per page.
///
/// For details on assembly stories, see [How to use storytelling to fit inline
/// assembly into Rust].
///
/// [How to use storytelling to fit inline assembly into Rust]: https://www.ralfj.de/blog/2026/03/13/inline-asm.html
#[inline]
pub fn init_buf(buf: &mut [MaybeUninit<u8>]) -> &mut [u8] {
	// SAFETY: The story of this assembly block is that it writes arbitrary
	// data into the buffer, initializing it. On Hermit, there is currently no
	// way for never-written-to allocated memory contents to change. Thus,
	// doing nothing is currently sufficient for writing arbitrary data.
	unsafe {
		core::arch::asm!(
			"/* {} {} */",
			in(reg) buf.as_mut_ptr(),
			in(reg) buf.len(),
			options(preserves_flags, nostack)
		);
	}

	// SAFETY: We have just provided a story that initializes the slice.
	unsafe { buf.assume_init_mut() }
}
