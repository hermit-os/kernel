use core::mem::MaybeUninit;

pub(super) static mut STACK: MaybeUninit<Stack> = MaybeUninit::uninit();

#[repr(C, align(0x1000))]
pub(super) struct Stack([u8; 0x8000]);
