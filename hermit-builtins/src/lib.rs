#![no_std]
#![no_main]
#![feature(linkage)]
#![cfg_attr(feature = "masos", feature(macro_metavar_expr_concat))]
#![cfg_attr(feature = "masos", feature(thread_local))]

#[cfg(feature = "masos")]
extern crate alloc;

#[cfg(feature = "masos")]
mod masos;
pub mod math;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
	loop {}
}
