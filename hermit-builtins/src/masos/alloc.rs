use alloc::alloc::{alloc, alloc_zeroed, dealloc, realloc};
use core::alloc::Layout;
use core::ptr::NonNull;

use align_address::Align;
use spinning_top::RawSpinlock;
use talc::TalcLock;
use talc::base::Talc;
use talc::base::binning::Binning;
use talc::source::Source;

#[global_allocator]
static TALC: TalcLock<RawSpinlock, Malloc> = TalcLock::new(Malloc::new());

#[derive(Debug)]
struct Malloc {
	heap_end: Option<NonNull<u8>>,
}

impl Malloc {
	// From dlmalloc-rs. Also the size of a Wasm page.
	const GRANULARITY: usize = 64 * 1024;

	const fn new() -> Self {
		Self { heap_end: None }
	}
}

unsafe impl Send for Malloc {}

unsafe impl Source for Malloc {
	fn acquire<B: Binning>(talc: &mut Talc<Self, B>, layout: Layout) -> Result<(), ()> {
		let size = layout.size().align_up(Self::GRANULARITY);
		let mut base = talc
			.source
			.heap_end
			.map(NonNull::as_ptr)
			.unwrap_or_default();

		let ret = unsafe { super::sys_mmap(size, libc::PROT_READ | libc::PROT_WRITE, &mut base) };
		if ret != 0 {
			return Err(());
		}

		let top = unsafe { base.add(size) };
		let new_end = match talc.source.heap_end {
			None => unsafe { talc.claim(base, size) },
			Some(heap_end) => unsafe { Some(talc.extend(heap_end, top)) },
		};
		assert_eq!(new_end, NonNull::new(top));
		talc.source.heap_end = new_end;

		Ok(())
	}
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sys_alloc(size: usize, align: usize) -> *mut u8 {
	unsafe { alloc(layout_from_size_align(size, align)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sys_dealloc(ptr: *mut u8, size: usize, align: usize) {
	unsafe { dealloc(ptr, layout_from_size_align(size, align)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sys_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
	unsafe { alloc_zeroed(layout_from_size_align(size, align)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sys_realloc(
	ptr: *mut u8,
	size: usize,
	align: usize,
	new_size: usize,
) -> *mut u8 {
	unsafe { realloc(ptr, layout_from_size_align(size, align), new_size) }
}

/// Deprecated
#[unsafe(no_mangle)]
unsafe extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
	unsafe { alloc(layout_from_size_align(size, align)) }
}

/// Deprecated
#[unsafe(no_mangle)]
unsafe extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	unsafe { dealloc(ptr, layout_from_size_align(size, align)) }
}

unsafe fn layout_from_size_align(size: usize, align: usize) -> Layout {
	if cfg!(debug_assertions) {
		Layout::from_size_align(size, align).unwrap()
	} else {
		unsafe { Layout::from_size_align_unchecked(size, align) }
	}
}
