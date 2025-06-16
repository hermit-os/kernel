use core::ffi::c_char;

use num_enum::{IntoPrimitive, TryFromPrimitive};

use super::{sockaddr, socklen_t};
use crate::errno::Errno;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct addrinfo {
	ai_flags: i32,
	ai_family: i32,
	ai_socktype: i32,
	ai_protocol: i32,
	ai_addrlen: socklen_t,
	ai_canonname: *mut c_char,
	ai_addr: *mut sockaddr,
	ai_next: *mut addrinfo,
}

#[derive(TryFromPrimitive, IntoPrimitive, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(i32)]
enum Eai {
	Again = 2,
	Badflags = 3,
	Fail = 4,
	Family = 5,
	Memory = 6,
	Nodata = 7,
	Noname = 8,
	Service = 9,
	Socktype = 10,
	System = 11,
	Overflow = 14,
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getaddrinfo(
	_nodename: *const c_char,
	_servname: *const c_char,
	_hints: *const addrinfo,
	_res: *mut *mut addrinfo,
) -> i32 {
	-i32::from(Errno::Inval)
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_freeaddrinfo(_ai: *mut addrinfo) {}
