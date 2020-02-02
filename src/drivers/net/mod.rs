// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//pub mod rtl8139;
pub mod uhyve;

use alloc::boxed::Box;
use synch::semaphore::*;

static mut NIC: Option<Box<dyn NetworkInterface>> = None;
static NET_SEM: Semaphore = Semaphore::new(0);

pub fn init() -> Result<(), ()> {
	let nic = uhyve::init()?;
	unsafe {
		NIC = Some(nic);
	}

	info!("Network initialized!");

	Ok(())
}

pub trait NetworkInterface {
	/// check if the driver in polling mode
	fn is_polling(&self) -> bool;
	/// set driver in polling/non-polling mode
	fn set_polling(&mut self, mode: bool);
	/// get mac address
	fn get_mac_address(&self) -> [u8; 6];
}

#[no_mangle]
pub fn uhyve_is_polling() -> bool {
	unsafe {
		match &NIC {
			Some(nic) => nic.is_polling(),
			None => false,
		}
	}
}

#[no_mangle]
pub fn uhyve_set_polling(mode: bool) {
	unsafe {
		match &mut NIC {
			Some(nic) => nic.set_polling(mode),
			None => {}
		}
	}
}

#[no_mangle]
pub fn uhyve_netwait(millis: Option<u64>) {
	if uhyve_is_polling() == false {
		let wakeup_time = match millis {
			Some(ms) => Some(::arch::processor::get_timer_ticks() + ms * 1000),
			None => None,
		};
		NET_SEM.acquire(wakeup_time);
	}
}

#[no_mangle]
pub fn uhyve_get_mac_address() -> [u8; 6] {
	unsafe {
		match &NIC {
			Some(nic) => nic.get_mac_address(),
			None => [0; 6],
		}
	}
}
