pub mod kernel;
pub mod mm;
#[cfg(target_os = "none")]
pub mod start;

use crate::arch::mm::paging::ExceptionStackFrame;

#[inline(always)]
pub(crate) fn swapgs(_stack_frame: &ExceptionStackFrame) {}
