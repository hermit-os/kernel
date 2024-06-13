//! A bootstrap allocator based on a statically allocated buffer.

/// A pointer range that can only be compared against.
mod ptr_range {
	use core::ops::Range;
	use core::ptr::NonNull;

	/// A pointer range that can only be compared against.
	pub struct PtrRange<T> {
		inner: Range<NonNull<T>>,
	}

	// SAFETY: We never dereference, but only compare, pointers.
	unsafe impl<T> Send for PtrRange<T> {}
	unsafe impl<T> Sync for PtrRange<T> {}

	impl<T> PtrRange<T> {
		/// Returns `true` if the pointer range contains `ptr`.
		pub fn contains(&self, ptr: NonNull<T>) -> bool {
			self.inner.contains(&ptr)
		}
	}

	impl<T> From<Range<NonNull<T>>> for PtrRange<T> {
		fn from(value: Range<NonNull<T>>) -> Self {
			Self { inner: value }
		}
	}
}

use core::mem::MaybeUninit;
use core::ops::Range;
use core::ptr::NonNull;

use allocator_api2::alloc::{AllocError, Allocator, Layout};
use take_static::take_static;

use self::ptr_range::PtrRange;

/// A bootstrap allocator.
///
/// This allocator is generic over the internal allocator and can only be created once.
/// The bootstrap allocator provides the internal allocator with static memory.
///
/// This allocator tracks, which static memory it was using initially.
/// It can be queried whether a pointer belongs to it.
pub struct BootstrapAllocator<A> {
	ptr_range: PtrRange<u8>,
	allocator: A,
}

impl<A> Default for BootstrapAllocator<A>
where
	A: From<&'static mut [MaybeUninit<u8>]>,
{
	fn default() -> Self {
		let mem = {
			const SIZE: usize = 4 * 1024;
			const BYTE: MaybeUninit<u8> = MaybeUninit::uninit();
			take_static! {
				/// The actual memory of the boostrap allocator.
				static MEM: [MaybeUninit<u8>; SIZE] = [BYTE; SIZE];
			}
			MEM.take().unwrap()
		};

		let ptr_range = {
			let Range { start, end } = mem.as_mut_ptr_range();
			let start = NonNull::new(start).unwrap().cast::<u8>();
			let end = NonNull::new(end).unwrap().cast::<u8>();
			PtrRange::from(start..end)
		};
		let allocator = A::from(mem);

		Self {
			ptr_range,
			allocator,
		}
	}
}

impl<A> BootstrapAllocator<A> {
	/// Returns `true` if the pointer belonged to the static memory of this allocator.
	pub fn manages(&self, ptr: NonNull<u8>) -> bool {
		self.ptr_range.contains(ptr)
	}
}

unsafe impl<A> Allocator for BootstrapAllocator<A>
where
	A: Allocator,
{
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		self.allocator.allocate(layout)
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		debug_assert!(self.manages(ptr));
		unsafe { self.allocator.deallocate(ptr, layout) }
	}
}
