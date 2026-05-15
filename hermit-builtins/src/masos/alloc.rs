use alloc::alloc::{alloc, alloc_zeroed, dealloc, realloc};
use core::alloc::Layout;
use core::mem::MaybeUninit;

use spinning_top::RawSpinlock;
use talc::TalcLock;
use talc::source::Claim;

#[global_allocator]
static TALC: TalcLock<RawSpinlock, Claim> = TalcLock::new(unsafe {
	/// 16 MiB of statically allocated Heap memory.
	#[repr(C, align(0x1000))]
	struct Heap([MaybeUninit<u8>; 0x100_0000]);

	static mut HEAP: Heap = Heap([MaybeUninit::uninit(); _]);

	let base = (&raw mut HEAP).cast::<u8>();
	let size = size_of::<Heap>();
	Claim::new(base, size)
});

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
