pub mod kernel;
pub mod mm;

#[allow(dead_code)]
#[inline(always)]
pub(crate) fn memory_barrier() {
	riscv::asm::sfence_vma_all();
}
