#![allow(dead_code)]
#![allow(nonstandard_style)]
use core::ffi::c_void;

use crate::fd::socket::*;
use crate::syscalls::__sys_write;

pub const AF_INET: i32 = 0;
pub const AF_INET6: i32 = 1;
pub const IPPROTO_IP: i32 = 0;
pub const IPPROTO_IPV6: i32 = 41;
pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;
pub const IPV6_ADD_MEMBERSHIP: i32 = 12;
pub const IPV6_DROP_MEMBERSHIP: i32 = 13;
pub const IPV6_MULTICAST_LOOP: i32 = 19;
pub const IPV6_V6ONLY: i32 = 27;
pub const IP_TTL: i32 = 2;
pub const IP_MULTICAST_TTL: i32 = 5;
pub const IP_MULTICAST_LOOP: i32 = 7;
pub const IP_ADD_MEMBERSHIP: i32 = 3;
pub const IP_DROP_MEMBERSHIP: i32 = 4;
pub const SHUT_RD: i32 = 0;
pub const SHUT_WR: i32 = 1;
pub const SHUT_RDWR: i32 = 2;
pub const SOCK_DGRAM: i32 = 2;
pub const SOCK_STREAM: i32 = 1;
pub const SOL_SOCKET: i32 = 4095;
pub const SO_BROADCAST: i32 = 32;
pub const SO_ERROR: i32 = 4103;
pub const SO_RCVTIMEO: i32 = 4102;
pub const SO_REUSEADDR: i32 = 4;
pub const SO_SNDTIMEO: i32 = 4101;
pub const SO_LINGER: i32 = 128;
pub const TCP_NODELAY: i32 = 1;
pub const MSG_PEEK: i32 = 1;
pub const FIONBIO: i32 = 0x8008667eu32 as i32;
pub const EAI_NONAME: i32 = -2200;
pub const EAI_SERVICE: i32 = -2201;
pub const EAI_FAIL: i32 = -2202;
pub const EAI_MEMORY: i32 = -2203;
pub const EAI_FAMILY: i32 = -2204;
pub type sa_family_t = u8;
pub type socklen_t = u32;
pub type in_addr_t = u32;
pub type in_port_t = u16;
pub type time_t = i64;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct in_addr {
	pub s_addr: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct in6_addr {
	pub s6_addr: [u8; 16],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sockaddr {
	pub sa_len: u8,
	pub sa_family: sa_family_t,
	pub sa_data: [u8; 14],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sockaddr_in {
	pub sin_len: u8,
	pub sin_family: sa_family_t,
	pub sin_port: in_port_t,
	pub sin_addr: in_addr,
	pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sockaddr_in6 {
	pub sin6_family: sa_family_t,
	pub sin6_port: in_port_t,
	pub sin6_addr: in6_addr,
	pub sin6_flowinfo: u32,
	pub sin6_scope_id: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ip_mreq {
	pub imr_multiaddr: in_addr,
	pub imr_interface: in_addr,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ipv6_mreq {
	pub ipv6mr_multiaddr: in6_addr,
	pub ipv6mr_interface: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct addrinfo {
	pub ai_flags: i32,
	pub ai_family: i32,
	pub ai_socktype: i32,
	pub ai_protocol: i32,
	pub ai_addrlen: socklen_t,
	pub ai_addr: *mut sockaddr,
	pub ai_canonname: *mut u8,
	pub ai_next: *mut addrinfo,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct linger {
	pub l_onoff: i32,
	pub l_linger: i32,
}

#[no_mangle]
pub extern "C" fn sys_socket(domain: i32, type_: i32, protocol: i32) -> i32 {
	kernel_function!(__sys_socket(domain, type_, protocol))
}

#[no_mangle]
pub extern "C" fn sys_accept(s: i32, addr: *mut sockaddr, addrlen: *mut socklen_t) -> i32 {
	kernel_function!(__sys_accept(s, addr, addrlen))
}

#[no_mangle]
pub extern "C" fn sys_listen(s: i32, backlog: i32) -> i32 {
	kernel_function!(__sys_listen(s, backlog))
}

#[no_mangle]
pub extern "C" fn sys_bind(s: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	kernel_function!(__sys_bind(s, name, namelen))
}

#[no_mangle]
pub extern "C" fn sys_connect(s: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	kernel_function!(__sys_connect(s, name, namelen))
}

#[no_mangle]
pub extern "C" fn sys_getsockname(s: i32, name: *mut sockaddr, namelen: *mut socklen_t) -> i32 {
	kernel_function!(__sys_getsockname(s, name, namelen))
}

#[no_mangle]
pub extern "C" fn sys_setsockopt(
	s: i32,
	level: i32,
	optname: i32,
	optval: *const c_void,
	optlen: socklen_t,
) -> i32 {
	kernel_function!(__sys_setsockopt(s, level, optname, optval, optlen))
}

#[no_mangle]
pub extern "C" fn getsockopt(
	s: i32,
	level: i32,
	optname: i32,
	optval: *mut c_void,
	optlen: *mut socklen_t,
) -> i32 {
	kernel_function!(__sys_getsockopt(s, level, optname, optval, optlen))
}

#[no_mangle]
pub extern "C" fn sys_getpeername(s: i32, name: *mut sockaddr, namelen: *mut socklen_t) -> i32 {
	kernel_function!(__sys_getpeername(s, name, namelen))
}

#[no_mangle]
pub extern "C" fn sys_freeaddrinfo(ai: *mut addrinfo) {
	kernel_function!(__sys_freeaddrinfo(ai))
}

#[no_mangle]
pub extern "C" fn sys_getaddrinfo(
	nodename: *const u8,
	servname: *const u8,
	hints: *const addrinfo,
	res: *mut *mut addrinfo,
) -> i32 {
	kernel_function!(__sys_getaddrinfo(nodename, servname, hints, res))
}

#[no_mangle]
pub extern "C" fn sys_send(s: i32, mem: *const c_void, len: usize, _flags: i32) -> isize {
	kernel_function!(__sys_write(s, mem as *const u8, len))
}

#[no_mangle]
pub extern "C" fn sys_shutdown_socket(s: i32, how: i32) -> i32 {
	kernel_function!(__sys_shutdown_socket(s, how))
}

#[no_mangle]
pub extern "C" fn sys_recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize {
	if flags == 0 {
		kernel_function!(__sys_recv(fd, buf, len))
	} else {
		(-crate::errno::EINVAL).try_into().unwrap()
	}
}

#[no_mangle]
pub extern "C" fn sys_sendto(
	socket: i32,
	buf: *const u8,
	len: usize,
	flags: i32,
	addr: *const sockaddr,
	addrlen: socklen_t,
) -> isize {
	kernel_function!(__sys_sendto(socket, buf, len, flags, addr, addrlen))
}

#[no_mangle]
pub extern "C" fn sys_recvfrom(
	socket: i32,
	buf: *mut u8,
	len: usize,
	flags: i32,
	addr: *mut sockaddr,
	addrlen: *mut socklen_t,
) -> isize {
	kernel_function!(__sys_recvfrom(socket, buf, len, flags, addr, addrlen))
}
