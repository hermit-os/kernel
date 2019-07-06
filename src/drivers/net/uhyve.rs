// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::collections::BTreeMap;
use core::ptr::read_volatile;
use core::str;
use core::sync::atomic::{AtomicBool, Ordering};

use smoltcp::iface::{EthernetInterfaceBuilder, NeighborCache, Routes};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};

use drivers::net::{networkd, NETWORK_TASK_ID};
use scheduler;

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

static POOLING: AtomicBool = AtomicBool::new(false);

fn is_pooling() -> bool {
	POOLING.load(Ordering::SeqCst)
}

const UHYVE_IRQ_NET: u32 = 11;
const UHYVE_PORT_NETINFO: u16 = 0x600;
const UHYVE_PORT_NETWRITE: u16 = 0x640;
const UHYVE_PORT_NETREAD: u16 = 0x680;
//const UHYVE_PORT_NETSTAT: u16   = 0x700;
const UHYVE_MAX_MSG_SIZE: usize = 1792;

/// Data type to determine the mac address
#[derive(Debug, Default)]
#[repr(C)]
struct UhyveNetinfo {
	/// mac address
	pub mac: [u8; 18],
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

extern "C" fn uhyve_thread(_arg: usize) {
	debug!("Hello from network thread!");

	::arch::irq::disable();

	let info: UhyveNetinfo = UhyveNetinfo::default();

	unsafe {
		outl(
			UHYVE_PORT_NETINFO,
			virt_to_phys(&info as *const _ as usize) as u32,
		);
	}
	let mac_str = str::from_utf8(&info.mac).unwrap();

	let neighbor_cache = NeighborCache::new(BTreeMap::new());
	let ethernet_addr = EthernetAddress([
		u8::from_str_radix(&mac_str[0..2], 16).unwrap(),
		u8::from_str_radix(&mac_str[3..5], 16).unwrap(),
		u8::from_str_radix(&mac_str[6..8], 16).unwrap(),
		u8::from_str_radix(&mac_str[9..11], 16).unwrap(),
		u8::from_str_radix(&mac_str[12..14], 16).unwrap(),
		u8::from_str_radix(&mac_str[15..17], 16).unwrap(),
	]);
	let hcip = get_ip();
	let ip_addrs = [IpCidr::new(
		IpAddress::v4(hcip[0], hcip[1], hcip[2], hcip[3]),
		24,
	)];
	let hcgw = get_gateway();
	let default_gw = Ipv4Address::new(hcgw[0], hcgw[1], hcgw[2], hcgw[3]);
	let mut routes_storage = [None; 1];
	let mut routes = Routes::new(&mut routes_storage[..]);
	routes.add_default_ipv4_route(default_gw).unwrap();
	let device = UhyveNet::new();

	info!("MAC address {}", ethernet_addr);
	info!("Configure network interface with address {}", ip_addrs[0]);
	info!("Configure gatway with address {}", default_gw);

	let mut iface = EthernetInterfaceBuilder::new(device)
		.ethernet_addr(ethernet_addr)
		.neighbor_cache(neighbor_cache)
		.ip_addrs(ip_addrs)
		.routes(routes)
		.finalize();

	// Install interrupt handler
	irq_install_handler(UHYVE_IRQ_NET, uhyve_irqhandler as usize);

	::arch::irq::enable();

	networkd(&mut iface, is_pooling);
}

pub fn init() -> Result<(), ()> {
	// does uhyve configure the network interface?
	let ip = get_ip();
	if ip[0] == 0xff && ip[1] == 0xff && ip[2] == 0xff && ip[3] == 0xff {
		return Err(());
	}

	let core_scheduler = core_scheduler();
	unsafe {
		NETWORK_TASK_ID = core_scheduler.spawn(
			uhyve_thread,
			0,
			scheduler::task::HIGH_PRIO,
			Some(crate::arch::mm::virtualmem::task_heap_start()),
		);
	}

	Ok(())
}

#[cfg(target_arch = "x86_64")]
extern "x86-interrupt" fn uhyve_irqhandler(_stack_frame: &mut ExceptionStackFrame) {
	debug!("Receive network interrupt from uhyve");
	POOLING.store(true, Ordering::SeqCst);
	crate::drivers::net::NET_SEM.release();
	apic::eoi();
	core_scheduler().scheduler();
}

/// A network device for uhyve.
#[derive(Debug)]
pub struct UhyveNet {
	mtu: usize,
}

impl UhyveNet {
	/// Creates a network device for uhyve.
	///
	/// Every packet transmitted through this device will be received through it
	/// in FIFO order.
	pub fn new() -> UhyveNet {
		UhyveNet { mtu: 1500 }
	}
}

impl<'a> Device<'a> for UhyveNet {
	type RxToken = RxToken;
	type TxToken = TxToken;

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = self.mtu;
		cap
	}

	fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
		let mut rx = RxToken::new();
		let data = UhyveRead::new(
			virt_to_phys(rx.buffer.as_mut_ptr() as usize),
			UHYVE_MAX_MSG_SIZE,
		);
		unsafe {
			outl(
				UHYVE_PORT_NETREAD,
				virt_to_phys(&data as *const _ as usize) as u32,
			);
		}

		if data.ret() == 0 {
			let tx = TxToken::new();
			rx.resize(data.len());
			trace!("resize message to {} bytes", rx.len());

			Some((rx, tx))
		} else {
			POOLING.store(false, Ordering::SeqCst);
			None
		}
	}

	fn transmit(&'a mut self) -> Option<Self::TxToken> {
		trace!("create TxToken to transfer data");
		Some(TxToken::new())
	}
}

#[doc(hidden)]
pub struct RxToken {
	buffer: [u8; UHYVE_MAX_MSG_SIZE],
	len: usize,
}

impl RxToken {
	pub fn new() -> RxToken {
		RxToken {
			buffer: [0; UHYVE_MAX_MSG_SIZE],
			len: UHYVE_MAX_MSG_SIZE,
		}
	}

	pub fn resize(&mut self, len: usize) {
		if len <= UHYVE_MAX_MSG_SIZE {
			self.len = len;
		} else {
			error!("Invalid message size {}", len);
		}
	}

	pub fn len(&self) -> usize {
		self.len
	}
}

impl phy::RxToken for RxToken {
	fn consume<R, F>(self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&[u8]) -> smoltcp::Result<R>,
	{
		let (first, _) = self.buffer.split_at(self.len);
		f(first)
	}
}

#[doc(hidden)]
pub struct TxToken;

impl TxToken {
	pub const fn new() -> Self {
		TxToken {}
	}

	fn write(&self, data: usize, len: usize) -> usize {
		let uhyve_write = UhyveWrite::new(virt_to_phys(data), len);
		unsafe {
			outl(
				UHYVE_PORT_NETWRITE,
				virt_to_phys(&uhyve_write as *const _ as usize) as u32,
			);
		}

		let ret = uhyve_write.ret();
		if ret != 0 {
			error!("Unable to send message: {}", ret);
		}

		uhyve_write.len()
	}
}

impl phy::TxToken for TxToken {
	fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		let mut buffer = vec![0; len];
		let result = f(&mut buffer);
		if result.is_ok() == true {
			self.write(buffer.as_ptr() as usize, len);
		}
		result
	}
}
