// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//pub mod rtl8139;
pub mod uhyve;

use alloc::boxed::Box;
use core::ffi::c_void;
use synch::spinlock::SpinlockIrqSave;

static NIC: SpinlockIrqSave<Option<Box<dyn NetworkInterface>>> = SpinlockIrqSave::new(None);

pub fn init() -> Result<(), ()> {
	let nic = uhyve::init()?;
	*NIC.lock() = Some(nic);

	info!("Network initialized!");

	Ok(())
}

pub trait NetworkInterface {
	/// check if the driver in polling mode
	fn is_polling(&self) -> bool;
	/// set driver in polling/non-polling mode
	fn set_polling(&mut self, mode: bool);
	/// initialize network and returns basic network configuration
	fn init(
		&mut self,
		sem: *const c_void,
		ip: &mut [u8; 4],
		gateway: &mut [u8; 4],
		mac: &mut [u8; 18],
	) -> i32;
	/// read packet from network interface
	fn read(&mut self, buf: usize, len: usize) -> usize;
	/// writr packet to the network interface
	fn write(&self, buf: usize, len: usize) -> usize;
}

#[no_mangle]
pub extern "C" fn sys_network_init(
	sem: *const c_void,
	ip: &mut [u8; 4],
	gateway: &mut [u8; 4],
	mac: &mut [u8; 18],
) -> i32 {
	match &mut *NIC.lock() {
		Some(nic) => nic.init(sem, ip, gateway, mac),
		None => -1,
	}
}

#[no_mangle]
pub extern "C" fn sys_is_polling() -> bool {
	match &*NIC.lock() {
		Some(nic) => nic.is_polling(),
		None => false,
	}
}

#[no_mangle]
pub extern "C" fn sys_set_polling(mode: bool) {
	match &mut *NIC.lock() {
		Some(nic) => nic.set_polling(mode),
		None => {}
	}
}

#[no_mangle]
pub extern "C" fn sys_netread(buf: usize, len: usize) -> usize {
	match &mut *NIC.lock() {
		Some(nic) => nic.read(buf, len),
		None => 0,
	}
}

#[no_mangle]
pub extern "C" fn sys_netwrite(buf: usize, len: usize) -> usize {
	match &*NIC.lock() {
		Some(nic) => nic.write(buf, len),
		None => 0,
	}
}
