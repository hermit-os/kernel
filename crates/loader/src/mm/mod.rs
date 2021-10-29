pub mod allocator;

#[cfg(not(test))]
use core::alloc::Layout;

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
	panic!("[OOM] Allocation of {:?} failed", layout);
}
