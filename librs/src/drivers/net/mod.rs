// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod uhyve;

use smoltcp::time::Instant;
use smoltcp::socket::SocketSet;
use smoltcp::iface::EthernetInterface;
use smoltcp::phy::Device;

use synch::semaphore::*;
use scheduler::task::TaskId;

static NET_SEM: Semaphore = Semaphore::new(0);
static mut NETWORK_TASK_ID: TaskId = TaskId::from(0);

pub fn get_network_task_id() -> TaskId {
	unsafe {
		NETWORK_TASK_ID
	}
}

pub fn networkd<'b, 'c, 'e, DeviceT: for<'d> Device<'d>>(iface: &mut EthernetInterface<'b, 'c, 'e, DeviceT>) -> ! {
	let mut sockets = SocketSet::new(vec![]);
	let boot_time = crate::arch::get_boot_time();
	let mut counter: usize = 0;

	loop {
		let microseconds = ::arch::processor::get_timer_ticks() - boot_time;
		let timestamp = Instant::from_millis(microseconds as i64 / 1000i64);

		match iface.poll(&mut sockets, timestamp) {
			Ok(ready) => {
				if ready == true {
					trace!("receive message {}", counter);
				} else {
					match iface.poll_delay(&sockets, timestamp) {
						Some(duration) => {
							trace!("duration {}", duration);
							NET_SEM.acquire(Some(duration.millis()))
						},
						None => NET_SEM.acquire(None),
					};
				}
			},
			Err(e) => {
				debug!("poll error: {}", e);
			}
		}

		counter = counter+1;
	}
}