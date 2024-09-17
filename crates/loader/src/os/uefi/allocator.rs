use alloc::vec;
use core::alloc::{GlobalAlloc, Layout};
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};

use allocator_api2::alloc::Allocator;
use one_shot_mutex::OneShotMutex;

use crate::bump_allocator::BumpAllocator;

pub enum GlobalAllocator {
	Uefi,
	Bump(BumpAllocator),
}

pub struct LockedAllocator(OneShotMutex<GlobalAllocator>);

impl LockedAllocator {
	pub const fn uefi() -> Self {
		Self(OneShotMutex::new(GlobalAllocator::Uefi))
	}
}

unsafe impl GlobalAlloc for LockedAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		match &*self.0.lock() {
			GlobalAllocator::Uefi => unsafe { uefi::allocator::Allocator.alloc(layout) },
			GlobalAllocator::Bump(bump) => bump
				.allocate(layout)
				// FIXME: Use NonNull::as_mut_ptr once `slice_ptr_get` is stabilized
				// https://github.com/rust-lang/rust/issues/74265
				.map_or(ptr::null_mut(), |ptr| ptr.as_ptr().cast()),
		}
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		match &*self.0.lock() {
			GlobalAllocator::Uefi => unsafe { uefi::allocator::Allocator.dealloc(ptr, layout) },
			GlobalAllocator::Bump(bump) => unsafe {
				bump.deallocate(NonNull::new(ptr).unwrap(), layout)
			},
		}
	}
}

#[global_allocator]
static ALLOCATOR: LockedAllocator = LockedAllocator::uefi();

pub fn exit_boot_services() {
	assert!(matches!(*ALLOCATOR.0.lock(), GlobalAllocator::Uefi));

	let mem = vec![MaybeUninit::uninit(); 4096].leak();

	let bump = BumpAllocator::from(mem);

	*ALLOCATOR.0.lock() = GlobalAllocator::Bump(bump);
}
