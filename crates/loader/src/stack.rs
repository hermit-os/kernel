pub static mut STACK: Stack = Stack::new();

// Align to page size
#[repr(C, align(0x1000))]
pub struct Stack([u8; Self::SIZE]);

impl Stack {
	pub const SIZE: usize = 0x8000;

	pub const fn new() -> Self {
		Self([0; Self::SIZE])
	}
}

pub fn get_stack_ptr() -> *mut u8 {
	let stack_top = &raw mut STACK;
	// SAFETY: Pointing directly past the object is allowed
	let stack_bottom = unsafe { stack_top.add(1) };
	stack_bottom.cast::<u8>()
}
