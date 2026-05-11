#![no_std]
#![no_main]
#![feature(linkage)]

mod math;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
	loop {}
}
