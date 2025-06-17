use core::ffi::c_char;

use num_enum::{IntoPrimitive, TryFromPrimitive};

use super::{sockaddr, socklen_t};
use crate::errno::Errno;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct addrinfo {
	ai_flags: Ai,
	ai_family: i32,
	ai_socktype: i32,
	ai_protocol: i32,
	ai_addrlen: socklen_t,
	ai_canonname: *mut c_char,
	ai_addr: *mut sockaddr,
	ai_next: *mut addrinfo,
}

bitflags! {
	#[repr(transparent)]
	#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
	pub struct Ai: i32 {
		const PASSIVE = 0x001;
		const CANONNAME = 0x002;
		const NUMERICHOST = 0x004;
		const NUMERICSERV = 0x008;
		const ALL = 0x100;
		const ADDRCONFIG = 0x400;
		const V4MAPPED = 0x800;

		// The source may set any bits
		const _ = !0;
	}
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

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_gai_strerror(ecode: i32) -> *const c_char {
	let Ok(ecode) = Eai::try_from(ecode) else {
		return c"Unknown error".as_ptr();
	};

	let s = match ecode {
		Eai::Again => c"Try again",
		Eai::Badflags => c"Invalid flags",
		Eai::Fail => c"Non-recoverable error",
		Eai::Family => c"Unrecognized address family or invalid length",
		Eai::Memory => c"Out of memory",
		Eai::Nodata => c"Name has no usable address",
		Eai::Noname => c"Name does not resolve",
		Eai::Service => c"Unrecognized service",
		Eai::Socktype => c"Unrecognized socket type",
		Eai::System => c"System error",
		Eai::Overflow => c"Overflow",
	};

	s.as_ptr()
}
