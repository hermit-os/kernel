// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::boxed::Box;
use arch::irq;
use core::sync::atomic::{AtomicBool, Ordering};
use drivers::net::{NetworkInterface, NET_SEM};

#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::apic;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::irq::*;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::percore::core_scheduler;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::uhyve_get_ip;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::mm::paging::virt_to_phys;
#[cfg(target_arch = "x86_64")]
use x86::io::*;

const UHYVE_IRQ_NET: u32 = 11;
const UHYVE_PORT_NETINFO: u16 = 0x600;

/// Data type to determine the mac address
#[derive(Debug, Default)]
#[repr(C)]
struct UhyveNetinfo {
	/// mac address
	pub mac: [u8; 6],
}

pub struct UhyveNetwork {
	/// mac address
	mac: [u8; 6],
	/// is NIC in polling mode?
	polling: AtomicBool,
}

impl UhyveNetwork {
	pub const fn new(mac: &[u8; 6]) -> Self {
		UhyveNetwork {
			mac: *mac,
			polling: AtomicBool::new(true),
		}
	}
}

impl NetworkInterface for UhyveNetwork {
	fn is_polling(&self) -> bool {
		self.polling.load(Ordering::SeqCst)
	}

	fn set_polling(&mut self, mode: bool) {
		self.polling.store(mode, Ordering::SeqCst);
		if mode {
			NET_SEM.release();
		}
	}

	fn get_mac_address(&self) -> [u8; 6] {
		self.mac
	}
}

pub fn init() -> Result<Box<dyn NetworkInterface>, ()> {
	// does uhyve configure the network interface?
	let ip = uhyve_get_ip();
	if ip[0] == 0xff && ip[1] == 0xff && ip[2] == 0xff && ip[3] == 0xff {
		return Err(());
	}

	debug!("Initialize uhyve network interface!");

	irq::disable();

	let nic = {
		let info: UhyveNetinfo = UhyveNetinfo::default();

		unsafe {
			outl(
				UHYVE_PORT_NETINFO,
				virt_to_phys(&info as *const _ as usize) as u32,
			);
		}

		Box::new(UhyveNetwork::new(&info.mac))
	};

	// Install interrupt handler
	irq_install_handler(UHYVE_IRQ_NET, uhyve_irqhandler as usize);

	irq::enable();

	Ok(nic)
}

#[cfg(target_arch = "x86_64")]
extern "x86-interrupt" fn uhyve_irqhandler(_stack_frame: &mut ExceptionStackFrame) {
	debug!("Receive network interrupt from uhyve");
	crate::drivers::net::uhyve_set_polling(true);
	apic::eoi();
	core_scheduler().scheduler();
}
