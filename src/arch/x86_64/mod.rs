pub mod kernel;
pub mod mm;

#[cfg(feature = "common-os")]
use x86_64::registers::segmentation::SegmentSelector;

use crate::arch::mm::paging::ExceptionStackFrame;

/// Helper function to swap the GS register, if the user-space is
/// is interrupted.
#[cfg(feature = "common-os")]
#[inline(always)]
pub(crate) fn swapgs(stack_frame: &ExceptionStackFrame) {
	use core::arch::asm;
	if stack_frame.code_segment != SegmentSelector(8) {
		unsafe {
			asm!("swapgs", options(nomem, nostack, preserves_flags));
		}
	}
}

#[cfg(not(feature = "common-os"))]
#[inline(always)]
pub(crate) fn swapgs(_stack_frame: &ExceptionStackFrame) {}

/// Force strict CPU ordering, serializes load and store operations.
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn memory_barrier() {
	use core::arch::asm;
	unsafe {
		asm!("mfence", options(nostack, nomem, preserves_flags),);
	}
}

pub fn init_drivers() {
	// Initialize PCI Drivers
	#[cfg(feature = "pci")]
	crate::drivers::pci::init_drivers();
	#[cfg(all(not(feature = "pci"), any(feature = "tcp", feature = "udp")))]
	crate::arch::x86_64::kernel::mmio::init_drivers();
}
