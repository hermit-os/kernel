use core::ffi::c_char;

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

const EAI_AGAIN: i32 = 2;
const EAI_BADFLAGS: i32 = 3;
const EAI_FAIL: i32 = 4;
const EAI_FAMILY: i32 = 5;
const EAI_MEMORY: i32 = 6;
const EAI_NODATA: i32 = 7;
const EAI_NONAME: i32 = 8;
const EAI_SERVICE: i32 = 9;
const EAI_SOCKTYPE: i32 = 10;
const EAI_SYSTEM: i32 = 11;
const EAI_OVERFLOW: i32 = 14;

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
