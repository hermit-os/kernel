pub mod kernel;
pub mod mm;

#[cfg(feature = "common-os")]
use x86_64::registers::segmentation::SegmentSelector;

use crate::arch::mm::paging::ExceptionStackFrame;

/// Swap the GS register, if the user-space is is interrupted.
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
