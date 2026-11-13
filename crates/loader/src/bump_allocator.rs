//! A bump allocator.
//!
//! This is a simple allocator design which can only allocate and not deallocate.

use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr::NonNull;

use allocator_api2::alloc::{AllocError, Allocator, Layout};

/// A simple, `!Sync` implementation of a bump allocator.
///
/// This allocator manages the provided memory.
pub struct BumpAllocator {
	mem: Cell<&'static mut [MaybeUninit<u8>]>,
}

unsafe impl Allocator for BumpAllocator {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		let ptr: *mut [MaybeUninit<u8>] = self.allocate_slice(layout)?;
		Ok(NonNull::new(ptr as *mut [u8]).unwrap())
	}

	unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {}
}

impl BumpAllocator {
	fn allocate_slice(&self, layout: Layout) -> Result<&'static mut [MaybeUninit<u8>], AllocError> {
		let mem = self.mem.take();
		let align_offset = mem.as_ptr().align_offset(layout.align());
		let mid = layout.size() + align_offset;
		if mid > mem.len() {
			self.mem.set(mem);
			Err(AllocError)
		} else {
			let (alloc, remaining) = mem.split_at_mut(mid);
			self.mem.set(remaining);
			Ok(&mut alloc[align_offset..])
		}
	}
}

impl From<&'static mut [MaybeUninit<u8>]> for BumpAllocator {
	fn from(mem: &'static mut [MaybeUninit<u8>]) -> Self {
		Self {
			mem: Cell::new(mem),
		}
	}
}
