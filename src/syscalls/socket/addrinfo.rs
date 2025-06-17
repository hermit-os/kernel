use alloc::boxed::Box;
use core::ffi::{CStr, c_char};
use core::{fmt, ptr};

use num_enum::{IntoPrimitive, TryFromPrimitive, TryFromPrimitiveError};

use super::{Af, Ipproto, Sock, SockFlags, sockaddr, sockaddrRef, socklen_t};
use crate::errno::Errno;

#[repr(C)]
struct addrinfo {
	ai_flags: Ai,
	ai_family: i32,
	ai_socktype: i32,
	ai_protocol: i32,
	ai_addrlen: socklen_t,
	ai_canonname: *mut c_char,
	ai_addr: *mut sockaddr,
	ai_next: Option<Box<addrinfo>>,
}

impl addrinfo {
	fn ai_family(&self) -> Option<Af> {
		let ai_family = u8::try_from(self.ai_family).ok()?;
		Af::try_from(ai_family).ok()
	}

	fn ai_socktype(&self) -> Option<(Sock, SockFlags)> {
		Sock::from_bits(self.ai_socktype)
	}

	fn ai_protocol(&self) -> Option<Ipproto> {
		let ai_protocol = u8::try_from(self.ai_protocol).ok()?;
		Ipproto::try_from(ai_protocol).ok()
	}

	fn ai_addr(&self) -> Option<Result<sockaddrRef<'_>, TryFromPrimitiveError<Af>>> {
		if self.ai_addr.is_null() {
			return None;
		}

		let ai_addr = unsafe { &*ptr::from_ref(&self.ai_addr).cast() };
		let ret = unsafe { sockaddr::as_ref(ai_addr) };
		Some(ret)
	}

	fn ai_canonname(&self) -> Option<&CStr> {
		if self.ai_canonname.is_null() {
			return None;
		}

		let ai_canonname = unsafe { CStr::from_ptr(self.ai_canonname) };
		Some(ai_canonname)
	}
}

impl fmt::Debug for addrinfo {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("addrinfo")
			.field("ai_flags", &self.ai_flags)
			.field("ai_family", &self.ai_family())
			.field("ai_socktype", &self.ai_socktype())
			.field("ai_protocol", &self.ai_protocol())
			.field("ai_addrlen", &self.ai_addrlen)
			.field("ai_addr", &self.ai_addr())
			.field("ai_canonname", &self.ai_canonname())
			.finish()
	}
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
