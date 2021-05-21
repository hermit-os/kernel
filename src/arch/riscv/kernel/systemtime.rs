use crate::arch::riscv::kernel::BOOT_INFO;

pub fn get_boot_time() -> u64 {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).boot_gtod) }
}
