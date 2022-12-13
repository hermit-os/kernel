//! Copied from `linked-list-allocator = "0.6.4"`

#[cfg(not(target_os = "none"))]
#[cfg(test)]
mod tests {
	use core::alloc::Layout;
	use std::mem::{align_of, size_of};
	use std::prelude::v1::*;

	use align_address::Align;

	use super::*;
	use crate::mm::allocator::*;
	use crate::mm::hole::*;

	fn new_heap() -> Heap {
		const HEAP_SIZE: usize = 1000;
		let heap_space = Box::into_raw(Box::new([0u8; HEAP_SIZE]));

		let heap = unsafe { Heap::new(heap_space as usize, HEAP_SIZE) };
		assert!(heap.bottom == heap_space as usize);
		assert!(heap.size == HEAP_SIZE);
		heap
	}

	fn new_max_heap() -> Heap {
		const HEAP_SIZE: usize = 1024;
		const HEAP_SIZE_MAX: usize = 2048;
		let heap_space = Box::into_raw(Box::new([0u8; HEAP_SIZE_MAX]));

		let heap = unsafe { Heap::new(heap_space as usize, HEAP_SIZE) };
		assert!(heap.bottom == heap_space as usize);
		assert!(heap.size == HEAP_SIZE);
		heap
	}

	#[test]
	fn empty() {
		let mut heap = Heap::empty();
		let layout = Layout::from_size_align(1, 1).unwrap();
		assert!(heap.allocate_first_fit(layout.clone()).is_err());
	}

	#[test]
	fn oom() {
		let mut heap = new_heap();
		let layout = Layout::from_size_align(heap.size() + 1, align_of::<usize>());
		let addr = heap.allocate_first_fit(layout.unwrap());
		assert!(addr.is_err());
	}

	#[test]
	fn allocate_double_usize() {
		let mut heap = new_heap();
		let size = size_of::<usize>() * 2;
		let layout = Layout::from_size_align(size, align_of::<usize>());
		let addr = heap.allocate_first_fit(layout.unwrap());
		assert!(addr.is_ok());
		let addr = addr.unwrap().as_ptr() as usize;
		assert!(addr == heap.bottom);
		let (hole_addr, hole_size) = heap.holes.first_hole().expect("ERROR: no hole left");
		assert!(hole_addr == heap.bottom + size);
		assert!(hole_size == heap.size - size);

		unsafe {
			assert_eq!((*((addr + size) as *const Hole)).size, heap.size - size);
		}
	}

	#[test]
	fn allocate_and_free_double_usize() {
		let mut heap = new_heap();

		let layout = Layout::from_size_align(size_of::<usize>() * 2, align_of::<usize>()).unwrap();
		let x = heap.allocate_first_fit(layout.clone()).unwrap();
		unsafe {
			*(x.as_ptr() as *mut (usize, usize)) = (0xdeafdeadbeafbabe, 0xdeafdeadbeafbabe);

			heap.deallocate(x, layout.clone());
			assert_eq!((*(heap.bottom as *const Hole)).size, heap.size);
			assert!((*(heap.bottom as *const Hole)).next.is_none());
		}
	}

	#[test]
	fn deallocate_right_before() {
		let mut heap = new_heap();
		let layout = Layout::from_size_align(size_of::<usize>() * 5, 1).unwrap();

		let x = heap.allocate_first_fit(layout.clone()).unwrap();
		let y = heap.allocate_first_fit(layout.clone()).unwrap();
		let z = heap.allocate_first_fit(layout.clone()).unwrap();

		unsafe {
			heap.deallocate(y, layout.clone());
			assert_eq!((*(y.as_ptr() as *const Hole)).size, layout.size());
			heap.deallocate(x, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, layout.size() * 2);
			heap.deallocate(z, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, heap.size);
		}
	}

	#[test]
	fn deallocate_right_behind() {
		let mut heap = new_heap();
		let size = size_of::<usize>() * 5;
		let layout = Layout::from_size_align(size, 1).unwrap();

		let x = heap.allocate_first_fit(layout.clone()).unwrap();
		let y = heap.allocate_first_fit(layout.clone()).unwrap();
		let z = heap.allocate_first_fit(layout.clone()).unwrap();

		unsafe {
			heap.deallocate(x, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, size);
			heap.deallocate(y, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, size * 2);
			heap.deallocate(z, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, heap.size);
		}
	}

	#[test]
	fn deallocate_middle() {
		let mut heap = new_heap();
		let size = size_of::<usize>() * 5;
		let layout = Layout::from_size_align(size, 1).unwrap();

		let x = heap.allocate_first_fit(layout.clone()).unwrap();
		let y = heap.allocate_first_fit(layout.clone()).unwrap();
		let z = heap.allocate_first_fit(layout.clone()).unwrap();
		let a = heap.allocate_first_fit(layout.clone()).unwrap();

		unsafe {
			heap.deallocate(x, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, size);
			heap.deallocate(z, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, size);
			assert_eq!((*(z.as_ptr() as *const Hole)).size, size);
			heap.deallocate(y, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, size * 3);
			heap.deallocate(a, layout.clone());
			assert_eq!((*(x.as_ptr() as *const Hole)).size, heap.size);
		}
	}

	#[test]
	fn reallocate_double_usize() {
		let mut heap = new_heap();

		let layout = Layout::from_size_align(size_of::<usize>() * 2, align_of::<usize>()).unwrap();

		let x = heap.allocate_first_fit(layout.clone()).unwrap();
		unsafe {
			heap.deallocate(x, layout.clone());
		}

		let y = heap.allocate_first_fit(layout.clone()).unwrap();
		unsafe {
			heap.deallocate(y, layout.clone());
		}

		assert_eq!(x, y);
	}

	#[test]
	fn allocate_multiple_sizes() {
		let mut heap = new_heap();
		let base_size = size_of::<usize>();
		let base_align = align_of::<usize>();

		let layout_1 = Layout::from_size_align(base_size * 2, base_align).unwrap();
		let layout_2 = Layout::from_size_align(base_size * 7, base_align).unwrap();
		let layout_3 = Layout::from_size_align(base_size * 3, base_align * 4).unwrap();
		let layout_4 = Layout::from_size_align(base_size * 4, base_align).unwrap();

		let x = heap.allocate_first_fit(layout_1.clone()).unwrap();
		let y = heap.allocate_first_fit(layout_2.clone()).unwrap();
		assert_eq!(y.as_ptr() as usize, x.as_ptr() as usize + base_size * 2);
		let z = heap.allocate_first_fit(layout_3.clone()).unwrap();
		assert_eq!(z.as_ptr() as usize % (base_size * 4), 0);

		unsafe {
			heap.deallocate(x, layout_1.clone());
		}

		let a = heap.allocate_first_fit(layout_4.clone()).unwrap();
		let b = heap.allocate_first_fit(layout_1.clone()).unwrap();
		assert_eq!(b, x);

		unsafe {
			heap.deallocate(y, layout_2);
			heap.deallocate(z, layout_3);
			heap.deallocate(a, layout_4);
			heap.deallocate(b, layout_1);
		}
	}

	#[test]
	fn allocate_usize() {
		let mut heap = new_heap();

		let layout = Layout::from_size_align(size_of::<usize>(), 1).unwrap();

		assert!(heap.allocate_first_fit(layout.clone()).is_ok());
	}

	#[test]
	fn allocate_usize_in_bigger_block() {
		let mut heap = new_heap();

		let layout_1 = Layout::from_size_align(size_of::<usize>() * 2, 1).unwrap();
		let layout_2 = Layout::from_size_align(size_of::<usize>(), 1).unwrap();

		let x = heap.allocate_first_fit(layout_1.clone()).unwrap();
		let y = heap.allocate_first_fit(layout_1.clone()).unwrap();
		unsafe {
			heap.deallocate(x, layout_1.clone());
		}

		let z = heap.allocate_first_fit(layout_2.clone());
		assert!(z.is_ok());
		let z = z.unwrap();
		assert_eq!(x, z);

		unsafe {
			heap.deallocate(y, layout_1.clone());
			heap.deallocate(z, layout_2);
		}
	}

	#[test]
	// see https://github.com/phil-opp/blog_os/issues/160
	fn align_from_small_to_big() {
		let mut heap = new_heap();

		let layout_1 = Layout::from_size_align(28, 4).unwrap();
		let layout_2 = Layout::from_size_align(8, 8).unwrap();

		// allocate 28 bytes so that the heap end is only 4 byte aligned
		assert!(heap.allocate_first_fit(layout_1.clone()).is_ok());
		// try to allocate a 8 byte aligned block
		assert!(heap.allocate_first_fit(layout_2.clone()).is_ok());
	}

	#[test]
	fn extend_empty_heap() {
		let mut heap = new_max_heap();

		unsafe {
			heap.extend(1024);
		}

		// Try to allocate full heap after extend
		let layout = Layout::from_size_align(2048, 1).unwrap();
		assert!(heap.allocate_first_fit(layout.clone()).is_ok());
	}

	#[test]
	fn extend_full_heap() {
		let mut heap = new_max_heap();

		let layout = Layout::from_size_align(1024, 1).unwrap();

		// Allocate full heap, extend and allocate again to the max
		assert!(heap.allocate_first_fit(layout.clone()).is_ok());
		unsafe {
			heap.extend(1024);
		}
		assert!(heap.allocate_first_fit(layout.clone()).is_ok());
	}

	#[test]
	fn extend_fragmented_heap() {
		let mut heap = new_max_heap();

		let layout_1 = Layout::from_size_align(512, 1).unwrap();
		let layout_2 = Layout::from_size_align(1024, 1).unwrap();

		let alloc1 = heap.allocate_first_fit(layout_1.clone());
		let alloc2 = heap.allocate_first_fit(layout_1.clone());

		assert!(alloc1.is_ok());
		assert!(alloc2.is_ok());

		unsafe {
			// Create a hole at the beginning of the heap
			heap.deallocate(alloc1.unwrap(), layout_1.clone());
		}

		unsafe {
			heap.extend(1024);
		}

		// We got additional 1024 bytes hole at the end of the heap
		// Try to allocate there
		assert!(heap.allocate_first_fit(layout_2.clone()).is_ok());
	}
}
