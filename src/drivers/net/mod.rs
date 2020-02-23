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

#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::apic;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::irq::*;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::percore::core_scheduler;

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
	/// get mac address
	fn get_mac_address(&self) -> [u8; 6];
}

#[no_mangle]
pub fn uhyve_netwakeup() {
	NET_SEM.release();
}

#[no_mangle]
pub fn uhyve_netwait(millis: Option<u64>) {
	match millis {
		Some(ms) => {
			if ms > 0 {
				let delay = Some(::arch::processor::get_timer_ticks() + ms * 1000);
				NET_SEM.acquire(delay);
			} else {
				NET_SEM.try_acquire();
			}
		}
		_ => {
			NET_SEM.acquire(None);
		}
	};
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

#[cfg(target_arch = "x86_64")]
extern "x86-interrupt" fn network_irqhandler(_stack_frame: &mut ExceptionStackFrame) {
	debug!("Receive network interrupt");
	apic::eoi();
	uhyve_netwakeup();
	core_scheduler().scheduler();
}
