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

#![allow(non_camel_case_types)]

use core::ptr;
use drivers::net::*;


extern "C" {
	pub fn etharp_output(netif: *mut netif, q: *mut pbuf, ipaddr: *const ip4_addr_t) -> err_t;
	pub fn ethernet_input(p: *mut pbuf, netif: *mut netif) -> err_t;
	pub fn ethip6_output(netif: *mut netif, q: *mut pbuf, ip6addr: *const ip6_addr_t) -> err_t;
	pub fn netif_create_ip6_linklocal_address(netif: *mut netif, from_mac_48bit: u8);
	pub fn netif_set_default(netif: *mut netif);
	pub fn netif_set_up(netif: *mut netif);
	pub fn netifapi_netif_add(netif: *mut netif, ipaddr: *const ip4_addr_t, netmask: *const ip4_addr_t, gw: *const ip4_addr_t, state: usize, init: netif_init_fn, input: netif_input_fn) -> err_t;
	pub fn netifapi_netif_common(netif: *mut netif, voidfunc: netifapi_void_fn, errtfunc: Option<netifapi_errt_fn>) -> err_t;
	pub fn pbuf_alloc(layer: pbuf_layer, length: u16, ty: pbuf_type) -> *mut pbuf;
	pub fn pbuf_header(p: *mut pbuf, header_size: i16) -> u8;
	pub fn sys_arch_sem_wait(sem: *mut sys_sem_t, timeout: u32) -> u32;
	pub fn sys_sem_free(sem: *mut sys_sem_t);
	pub fn sys_sem_new(sem: *mut sys_sem_t, count: u8) -> err_t;
	pub fn sys_sem_signal(sem: *mut sys_sem_t);
	pub fn tcpip_callback_with_block(function: tcpip_callback_fn, ctx: usize, block: u8) -> err_t;
	pub fn tcpip_init(tcpip_init_done: tcpip_init_done_fn, arg: usize);
}

pub type err_t = i8;
pub type ip4_addr_t = ip_addr_t;
pub type ip6_addr_t = ip_addr_t;
pub type netif_igmp_mac_filter_fn = extern "C" fn(*mut netif, *const ip4_addr_t, netif_mac_filter_action) -> err_t;
pub type netif_init_fn = unsafe extern "C" fn(*mut netif) -> err_t;
pub type netif_input_fn = unsafe extern "C" fn(*mut pbuf, *mut netif) -> err_t;
pub type netif_linkoutput_fn = unsafe extern "C" fn(*mut netif, *mut pbuf) -> err_t;
pub type netif_mac_filter_action = i32;
pub type netif_mld_mac_filter_fn = extern "C" fn(*mut netif, *const ip6_addr_t, netif_mac_filter_action) -> err_t;
pub type netif_output_fn = unsafe extern "C" fn(*mut netif, *mut pbuf, *const ip4_addr_t) -> err_t;
pub type netif_output_ip6_fn = unsafe extern "C" fn(*mut netif, *mut pbuf, *const ip6_addr_t) -> err_t;
pub type netifapi_errt_fn = unsafe extern "C" fn(*mut netif) -> err_t;
pub type netifapi_void_fn = unsafe extern "C" fn(*mut netif);
pub type sem_t = sys_mutex_t;
pub type sys_mutex_t = i8;
pub type tcpip_callback_fn = unsafe extern "C" fn(usize);
pub type tcpip_init_done_fn = extern "C" fn(usize);

pub const ETH_PAD_SIZE: i16 = 2;

pub const LWIP_IPV6_NUM_ADDRESSES: usize = 3;
pub const LWIP_NUM_NETIF_CLIENT_DATA: usize = 1;
pub const NETIF_MAX_HWADDR_LEN: usize = 6;

pub const NETIF_FLAG_BROADCAST: u8 = 0x02;
pub const NETIF_FLAG_LINK_UP:   u8 = 0x04;
pub const NETIF_FLAG_ETHARP:    u8 = 0x08;
pub const NETIF_FLAG_IGMP:      u8 = 0x20;
pub const NETIF_FLAG_MLD6:      u8 = 0x40;

#[allow(dead_code)]
#[repr(C)]
pub enum err_type_t {
	ERR_OK = 0,
	ERR_MEM = -1,
	ERR_BUF = -2,
	ERR_TIMEOUT = -3,
	ERR_RTE = -4,
	ERR_INPROGRESS = -5,
	ERR_VAL = -6,
	ERR_WOULDBLOCK = -7,
	ERR_USE = -8,
	ERR_ALREADY = -9,
	ERR_ISCONN = -10,
	ERR_CONN = -11,
	ERR_IF = -12,
	ERR_ABRT = -13,
	ERR_RST = -14,
	ERR_CLSD = -15,
	ERR_ARG = -16,
}
pub use self::err_type_t::*;

#[allow(dead_code)]
#[repr(C)]
pub enum lwip_internal_netif_client_data_index
{
	LWIP_NETIF_CLIENT_DATA_INDEX_DHCP,
	//LWIP_NETIF_CLIENT_DATA_INDEX_AUTOIP,
	LWIP_NETIF_CLIENT_DATA_INDEX_IGMP,
	LWIP_NETIF_CLIENT_DATA_INDEX_MLD6,
	LWIP_NETIF_CLIENT_DATA_INDEX_MAX,
}
pub use self::lwip_internal_netif_client_data_index::*;

#[allow(dead_code)]
#[repr(C)]
pub enum lwip_ip_addr_type {
	IPADDR_TYPE_V4 = 0,
	IPADDR_TYPE_V6 = 6,
	IPADDR_TYPE_ANY = 46,
}
pub use self::lwip_ip_addr_type::*;

#[allow(dead_code)]
#[repr(C)]
pub enum pbuf_layer {
	PBUF_TRANSPORT,
	PBUF_IP,
	PBUF_LINK,
	PBUF_RAW_TX,
	PBUF_RAW,
}
pub use self::pbuf_layer::*;

#[allow(dead_code)]
#[repr(C)]
pub enum pbuf_type {
	PBUF_RAM,
	PBUF_ROM,
	PBUF_REF,
	PBUF_POOL,
}
pub use self::pbuf_type::*;


/// lwIP's ip_addr_t structure for the combined IPv4 & IPv6 version.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ip_addr_t {
	pub addr: [u32; 4],
	pub ty: u8,
}

impl ip_addr_t {
	pub const fn new() -> Self {
		Self {
			addr: [0; 4],
			ty: IPADDR_TYPE_V4 as u8,
		}
	}

	pub const fn new_ip4(first: u8, second: u8, third: u8, fourth: u8) -> Self {
		Self {
			addr: [
				(first as u32) << 24 | (second as u32) << 16 | (third as u32) << 8 | (fourth as u32),
				0,
				0,
				0
			],
			ty: IPADDR_TYPE_V4 as u8,
		}
	}
}

/// lwIP's netif structure, with members undefined through lwipopts.h and opt.h commented out.
#[repr(C)]
pub struct netif {
	pub next: *mut netif,
	pub ip_addr: ip_addr_t,
	pub netmask: ip_addr_t,
	pub gw: ip_addr_t,
	pub ip6_addr: [ip_addr_t; LWIP_IPV6_NUM_ADDRESSES],
	pub ip6_addr_state: [u8; LWIP_IPV6_NUM_ADDRESSES],
	pub input: Option<netif_input_fn>,
	pub output: netif_output_fn,
	pub linkoutput: Option<netif_linkoutput_fn>,
	pub output_ip6: netif_output_ip6_fn,
	//pub status_callback: Option<netif_status_callback_fn>,
	//pub link_callback: Option<netif_status_callback_fn>,
	//pub remove_callback: Option<netif_status_callback_fn>,
	pub state: usize,
	pub client_data: [usize; LWIP_NETIF_CLIENT_DATA_INDEX_MAX as usize + LWIP_NUM_NETIF_CLIENT_DATA],
	pub ip6_autoconfig_enabled: u8,
	pub rs_count: u8,
	//pub hostname: *const u8,
	//pub chksum_flags: u16,
	pub mtu: u16,
	pub hwaddr_len: u8,
	pub hwaddr: [u8; NETIF_MAX_HWADDR_LEN],
	pub flags: u8,
	pub name: [u8; 2],
	pub num: u8,
	//pub link_type: u8,
	//pub link_speed: u32,
	//pub ts: u32,
	//pub mib2_counters: stats_mib2_netif_ctrs,
	pub igmp_mac_filter: Option<netif_igmp_mac_filter_fn>,
	pub mld_mac_filter: Option<netif_mld_mac_filter_fn>,
	//pub addr_hint: *mut u8,
	pub loop_first: *mut pbuf,
	pub loop_last: *mut pbuf,
}

#[repr(C)]
pub struct pbuf {
	pub next: *mut pbuf,
	pub payload: usize,
	pub tot_len: u16,
	pub len: u16,
	pub ty: u8,
	pub flags: u8,
	pub reference_count: u16,
}

#[repr(C)]
pub struct sys_sem_t {
	pub sem: sem_t,
	pub valid: i32,
}


pub struct NetworkInterface {
	is_receiving: bool,
	netif: netif,
}

impl NetworkInterface {
	pub const fn new(num: u8) -> Self {
		Self {
			is_receiving: false,
			netif: netif {
				next: 0 as *mut netif,
				ip_addr: ip_addr_t::new(),
				netmask: ip_addr_t::new(),
				gw: ip_addr_t::new(),
				ip6_addr: [ip_addr_t::new(); LWIP_IPV6_NUM_ADDRESSES],
				ip6_addr_state: [0; LWIP_IPV6_NUM_ADDRESSES],
				input: None,
				output: etharp_output,
				linkoutput: None,
				output_ip6: ethip6_output,
				state: 0,
				client_data: [0; LWIP_NETIF_CLIENT_DATA_INDEX_MAX as usize + LWIP_NUM_NETIF_CLIENT_DATA],
				ip6_autoconfig_enabled: 1,
				rs_count: 0,
				mtu: 1500,
				hwaddr_len: NETIF_MAX_HWADDR_LEN as u8,
				hwaddr: [0; NETIF_MAX_HWADDR_LEN],
				flags: NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_IGMP | NETIF_FLAG_LINK_UP | NETIF_FLAG_MLD6,
				name: [b'e', b'n'],
				num: num,
				igmp_mac_filter: None,
				mld_mac_filter: None,
				loop_first: 0 as *mut pbuf,
				loop_last: 0 as *mut pbuf,
			},
		}
	}

	pub fn init(&mut self, result: DetectionResult, ip: ip_addr_t, netmask: ip_addr_t, gateway: ip_addr_t) {
		self.netif.state = self as *mut Self as usize;

		let (pci_adapter, network_adapter_init_fn) = result.unwrap();
		pci_adapter.make_bus_master();
		network_adapter_init_fn(&mut self.netif, pci_adapter, ip, netmask, gateway);

		unsafe {
			assert!(netifapi_netif_common(&mut self.netif, netif_set_default, None) == ERR_OK as err_t);
			assert!(netifapi_netif_common(&mut self.netif, netif_set_up, None) == ERR_OK as err_t);
		}
	}

	pub fn is_receiving(&self) -> bool {
		self.is_receiving
	}

	pub unsafe fn receive_packet(&mut self, buffer: *const u8, length: usize) {
		assert!(self.is_receiving);

		// Allocate a new pbuf for the given packet length (plus padding).
		let padded_length = length as u16 + ETH_PAD_SIZE as u16;
		let p = pbuf_alloc(PBUF_RAW, padded_length, PBUF_POOL);
		assert!(!p.is_null());

		// Drop the padding word.
		pbuf_header(p, -ETH_PAD_SIZE);

		// Loop through the chain of pbufs forming a single packet
		// and copy the given packet into the chain.
		let mut current = p;
		let mut i = 0;
		while !current.is_null() {
			ptr::copy_nonoverlapping(buffer.offset(i), (*current).payload as *mut u8, (*current).len as usize);
			i += (*current).len as isize;
			current = (*current).next;
		}

		// Reclaim the padding word.
		pbuf_header(p, ETH_PAD_SIZE);

		// Forward the packet to lwIP.
		assert!(self.netif.input.unwrap()(p, &mut self.netif) == ERR_OK as err_t);
	}

	pub fn set_linkoutput_handler(&mut self, handler: netif_linkoutput_fn) {
		self.netif.linkoutput = Some(handler);
	}

	pub fn set_mac_address(&mut self, mac_address: [u8; NETIF_MAX_HWADDR_LEN]) {
		self.netif.hwaddr = mac_address;
		unsafe { netif_create_ip6_linklocal_address(&mut self.netif, 1); }
	}

	pub fn start_receiving(&mut self, handler: tcpip_callback_fn) {
		unsafe { assert!(tcpip_callback_with_block(handler, self as *mut Self as usize, 0) == ERR_OK as err_t); }
		self.is_receiving = true;
	}

	pub fn stop_receiving(&mut self) {
		self.is_receiving = false;
	}

	pub unsafe fn transmit_packet(buffer: *mut u8, p: *mut pbuf) {
		assert!(!p.is_null());

		// Drop the padding word.
		pbuf_header(p, -ETH_PAD_SIZE);

		// Loop through the chain of pbufs forming a single packet
		// and copy the entire packet into the given buffer.
		let mut current = p;
		let mut i = 0;
		while (*current).len != (*current).tot_len {
			ptr::copy_nonoverlapping((*current).payload as *mut u8, buffer.offset(i), (*current).len as usize);
			i += (*current).len as isize;
			current = (*current).next;
		}

		// Reclaim the padding word.
		pbuf_header(p, ETH_PAD_SIZE);
	}
}
