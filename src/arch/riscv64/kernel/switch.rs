use core::arch::global_asm;

global_asm!(include_str!("switch.s"));

extern "C" {
	pub fn switch_to_task(old_stack: *mut usize, new_stack: usize);
}
