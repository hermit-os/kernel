use core::cell::UnsafeCell;
use core::mem::{self, MaybeUninit};

#[repr(C, align(0x1000))]
pub struct Stack(UnsafeCell<[MaybeUninit<u8>; 0x1000]>);

unsafe impl Sync for Stack {}

impl Stack {
	const fn new() -> Self {
		let fill = if cfg!(debug_assertions) {
			MaybeUninit::new(0xcd)
		} else {
			MaybeUninit::uninit()
		};
		Self(UnsafeCell::new([fill; _]))
	}

	pub const fn top_offset() -> u16 {
		mem::size_of::<Self>() as u16 - 0x10
	}
}

pub static STACK: Stack = Stack::new();
