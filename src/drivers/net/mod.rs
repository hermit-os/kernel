// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod uhyve;
pub mod rtl8139;

use smoltcp::time::Instant;
use smoltcp::socket::SocketSet;
use smoltcp::iface::EthernetInterface;
use smoltcp::phy::Device;
//use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};
//use smoltcp::socket::{RawSocketBuffer, RawPacketMetadata};
//use smoltcp::dhcp::Dhcpv4Client;

use synch::semaphore::*;
use scheduler::task::TaskId;

static NET_SEM: Semaphore = Semaphore::new(0);
static mut NETWORK_TASK_ID: TaskId = TaskId::from(0);

pub fn get_network_task_id() -> TaskId {
	unsafe {
		NETWORK_TASK_ID
	}
}

/*pub fn networkd_with_dhcp<'b, 'c, 'e, DeviceT: for<'d> Device<'d>, F>(iface: &mut EthernetInterface<'b, 'c, 'e, DeviceT>, is_polling: F) -> !
	where F: Fn() -> bool {
	let dhcp_rx_buffer = RawSocketBuffer::new(
        [RawPacketMetadata::EMPTY; 1],
        vec![0; 900]
    );
    let dhcp_tx_buffer = RawSocketBuffer::new(
        [RawPacketMetadata::EMPTY; 1],
        vec![0; 600]
    );
	let mut sockets = SocketSet::new(vec![]);
	let boot_time = crate::arch::get_boot_time();
	let mut counter: usize = 0;
	let microseconds = ::arch::processor::get_timer_ticks() - boot_time;
	let timestamp = Instant::from_millis(microseconds as i64 / 1000i64);
	let mut dhcp = Dhcpv4Client::new(&mut sockets, dhcp_rx_buffer, dhcp_tx_buffer, timestamp);
    let mut prev_cidr = Ipv4Cidr::new(Ipv4Address::UNSPECIFIED, 0);

	loop {
		let microseconds = ::arch::processor::get_timer_ticks() - boot_time;
		let timestamp = Instant::from_millis(microseconds as i64 / 1000i64);
        iface.poll(&mut sockets, timestamp)
            .map(|_| {trace!("receive message {}", counter); counter = counter+1;})
            .unwrap_or_else(|e| info!("Poll: {:?}", e));
        let config = dhcp.poll(iface, &mut sockets, timestamp)
            .unwrap_or_else(|e| {
                debug!("DHCP: {:?}", e);
                None
            });
		config.map(|config| {
            info!("DHCP config: {:?}", config);
            match config.address {
                Some(cidr) => if cidr != prev_cidr {
                    iface.update_ip_addrs(|addrs| {
                        addrs.iter_mut().nth(0)
                            .map(|addr| {
                                *addr = IpCidr::Ipv4(cidr);
                            });
                    });
                    prev_cidr = cidr;
                    info!("Assigned a new IPv4 address: {}", cidr);
                }
                _ => {}
            }

            config.router.map(|router| iface.routes_mut()
                              .add_default_ipv4_route(router.into())
                              .unwrap()
            );
            iface.routes_mut()
                .update(|routes_map| {
                    routes_map.get(&IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0))
                        .map(|default_route| {
                            info!("Default gateway: {}", default_route.via_router);
                        });
                });

            if config.dns_servers.iter().any(|s| s.is_some()) {
                info!("DNS servers:");
                for dns_server in config.dns_servers.iter().filter_map(|s| *s) {
                    info!("- {}", dns_server);
                }
            }
        });

		if is_polling() == false {
			let mut timeout = dhcp.next_poll(timestamp);
			iface.poll_delay(&sockets, timestamp)
        	    .map(|sockets_timeout| timeout = sockets_timeout);
			debug!("networkd timeout {}", timeout.millis());

			// Calculate the absolute wakeup time in processor timer ticks out of the relative timeout in milliseconds.
        	let wakeup_time = if timeout.millis() > 0  {
                Some(::arch::processor::get_timer_ticks() + (timeout.millis() as u64) * 1000)
        	} else {
                Some(::arch::processor::get_timer_ticks() + 100)
        	};
			NET_SEM.acquire(wakeup_time);
		}
	}
}*/

pub fn networkd<'b, 'c, 'e, DeviceT: for<'d> Device<'d>, F>(iface: &mut EthernetInterface<'b, 'c, 'e, DeviceT>, is_polling: F) -> ! 
	where F: Fn() -> bool {
	let mut sockets = SocketSet::new(vec![]);
	let boot_time = crate::arch::get_boot_time();
	let mut counter: usize = 0;

	loop {
		let microseconds = ::arch::processor::get_timer_ticks() - boot_time;
		let timestamp = Instant::from_millis(microseconds as i64 / 1000i64);

		iface.poll(&mut sockets, timestamp)
            .map(|_| {trace!("receive message {}", counter); counter = counter+1;})
            .unwrap_or_else(|e| debug!("Poll: {:?}", e));
		
		if is_polling() == false {
			let wakeup_time = match iface.poll_delay(&sockets, timestamp) {
				Some(duration) => {
					// Calculate the absolute wakeup time in processor timer ticks out of the relative timeout in milliseconds.
        			if duration.millis() > 0  {
                		Some(::arch::processor::get_timer_ticks() + (duration.millis() as u64) * 1000)
        			} else {
                		Some(::arch::processor::get_timer_ticks() + 100)
        			}
				},
				None => None,
			};
			NET_SEM.acquire(wakeup_time);
		}
	}
}