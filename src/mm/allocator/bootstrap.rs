mod ptr_range {
	use core::ops::Range;
	use core::ptr::NonNull;

	pub struct PtrRange<T> {
		inner: Range<NonNull<T>>,
	}

	// SAFETY: We never dereference, but only compare, pointers.
	unsafe impl<T> Send for PtrRange<T> {}
	unsafe impl<T> Sync for PtrRange<T> {}

	impl<T> PtrRange<T> {
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

use core::alloc::{AllocError, Allocator, Layout};
use core::mem::MaybeUninit;
use core::ops::Range;
use core::ptr::NonNull;

use hermit_sync::ExclusiveCell;

use self::ptr_range::PtrRange;

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
			static MEM: ExclusiveCell<[MaybeUninit<u8>; SIZE]> = ExclusiveCell::new([BYTE; SIZE]);
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
