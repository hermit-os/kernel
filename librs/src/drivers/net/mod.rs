// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

pub mod e1000;
pub mod lwip;

use arch::pci;
use core::mem;
use self::lwip::*;

type DetectionResult = Option<(pci::PciAdapter, fn(&mut netif, pci::PciAdapter, ip_addr_t, ip_addr_t, ip_addr_t))>;


/// The first (and only) ethernet network interface we use in HermitCore.
static mut EN0: NetworkInterface = NetworkInterface::new(0);


extern "C" fn tcpip_init_done(arg: usize) {
	unsafe {
		let sem = arg as *mut sys_sem_t;
		sys_sem_signal(sem);

		// TODO: info! about task ID
	}
}

pub fn init() {
	let result = e1000::detect()
		.or_else(|| None);

	if result.is_none() {
		warn!("Found no network adapter, starting HermitCore without networking.");
		return;
	}

	// Initialize the lwIP TCP/IP Stack.
	unsafe {
		let mut sem: sys_sem_t = mem::uninitialized();
		assert!(sys_sem_new(&mut sem, 0) == ERR_OK as err_t, "Couldn't initialize lwIP semaphore");

		tcpip_init(tcpip_init_done, &mut sem as *mut sys_sem_t as usize);

		sys_arch_sem_wait(&mut sem, 0);
		info!("TCP/IP initialized.");
		sys_sem_free(&mut sem);
	}

	// Initialize the detected network adapter.
	let ip = ip_addr_t::new();
	let netmask = ip_addr_t::new();
	let gateway = ip_addr_t::new();
	unsafe { EN0.init(result, ip, netmask, gateway); }
}
