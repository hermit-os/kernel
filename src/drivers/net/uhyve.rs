// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::boxed::Box;
use arch::irq;
use core::ffi::c_void;
use core::ptr::read_volatile;
use core::sync::atomic::{AtomicBool, Ordering};
use core::{ptr, str};
use drivers::net::NetworkInterface;
use synch;
use syscalls::sys_sem_post;

#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::apic;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::irq::*;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::percore::core_scheduler;
#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::{get_gateway, get_ip};
#[cfg(target_arch = "x86_64")]
use arch::x86_64::mm::paging::virt_to_phys;
#[cfg(target_arch = "x86_64")]
use x86::io::*;

const UHYVE_IRQ_NET: u32 = 11;
const UHYVE_PORT_NETINFO: u16 = 0x600;
const UHYVE_PORT_NETWRITE: u16 = 0x640;
const UHYVE_PORT_NETREAD: u16 = 0x680;
//const UHYVE_PORT_NETSTAT: u16   = 0x700;

/// Data type to determine the mac address
#[derive(Debug, Default)]
#[repr(C)]
struct UhyveNetinfo {
	/// mac address
	pub mac: [u8; 18],
}

pub struct UhyveNetwork {
	/// Semaphore to block IP thread
	sem: *const c_void,
	/// mac address
	mac: [u8; 18],
	/// is NIC in polling mode?
	polling: AtomicBool,
}

impl UhyveNetwork {
	pub const fn new(mac: &[u8; 18]) -> Self {
		UhyveNetwork {
			sem: ptr::null(),
			mac: *mac,
			polling: AtomicBool::new(false),
		}
	}
}

impl NetworkInterface for UhyveNetwork {
	fn is_polling(&self) -> bool {
		self.polling.load(Ordering::SeqCst)
	}

	fn set_polling(&mut self, mode: bool) {
		self.polling.store(mode, Ordering::SeqCst);
		if mode && !self.sem.is_null() {
			sys_sem_post(self.sem as *const synch::semaphore::Semaphore);
		}
	}

	fn init(
		&mut self,
		sem: *const c_void,
		ip: &mut [u8; 4],
		gateway: &mut [u8; 4],
		mac: &mut [u8; 18],
	) -> i32 {
		let mac_str = str::from_utf8(&self.mac).unwrap();
		info!("MAC address: {}", mac_str);

		let myip = get_ip();
		let mygw = get_gateway();

		self.sem = sem;
		mac[0..].copy_from_slice(mac_str.as_bytes());
		ip.copy_from_slice(&myip);
		gateway.copy_from_slice(&mygw);

		0
	}

	fn write(&self, buf: usize, len: usize) -> usize {
		let uhyve_write = UhyveWrite::new(virt_to_phys(buf), len);

		let irq = irq::nested_disable();
		unsafe {
			outl(
				UHYVE_PORT_NETWRITE,
				virt_to_phys(&uhyve_write as *const _ as usize) as u32,
			);
		}
		irq::nested_enable(irq);

		let ret = uhyve_write.ret();
		if ret != 0 {
			error!("Unable to send message: {}", ret);
		}

		uhyve_write.len()
	}

	fn read(&mut self, buf: usize, len: usize) -> usize {
		let data = UhyveRead::new(virt_to_phys(buf), len);
		let irq = irq::nested_disable();
		unsafe {
			outl(
				UHYVE_PORT_NETREAD,
				virt_to_phys(&data as *const _ as usize) as u32,
			);
		}

		if data.ret() == 0 {
			trace!("resize message to {} bytes", data.len());

			irq::nested_enable(irq);
			data.len()
		} else {
			self.set_polling(false);
			irq::nested_enable(irq);
			0
		}
	}
}

/// Datatype to receive packets from uhyve
#[derive(Debug)]
#[repr(C)]
struct UhyveRead {
	/// address to the received data
	pub data: usize,
	/// length of the buffer
	pub len: usize,
	/// amount of received data (in bytes)
	pub ret: i32,
}

impl UhyveRead {
	pub fn new(data: usize, len: usize) -> Self {
		UhyveRead {
			data: data,
			len: len,
			ret: 0,
		}
	}

	pub fn len(&self) -> usize {
		unsafe { read_volatile(&self.len) }
	}

	pub fn ret(&self) -> i32 {
		unsafe { read_volatile(&self.ret) }
	}
}

/// Datatype to forward packets to uhyve
#[derive(Debug)]
#[repr(C)]
struct UhyveWrite {
	/// address to the data
	pub data: usize,
	/// length of the data
	pub len: usize,
	/// return value, transfered bytes
	pub ret: i32,
}

impl UhyveWrite {
	pub fn new(data: usize, len: usize) -> Self {
		UhyveWrite {
			data: data,
			len: len,
			ret: 0,
		}
	}

	pub fn ret(&self) -> i32 {
		unsafe { read_volatile(&self.ret) }
	}

	pub fn len(&self) -> usize {
		unsafe { read_volatile(&self.len) }
	}
}

pub fn init() -> Result<Box<dyn NetworkInterface>, ()> {
	// does uhyve configure the network interface?
	let ip = get_ip();
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
	crate::drivers::net::sys_set_polling(true);
	apic::eoi();
	core_scheduler().scheduler();
}
