use aarch64_cpu::asm::barrier::{ISH, dmb};

pub mod kernel;
pub mod mm;

/// Force strict CPU ordering, serializes load and store operations.
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn memory_barrier() {
	dmb(ISH);
}
