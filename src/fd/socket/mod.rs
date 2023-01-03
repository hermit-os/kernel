use alloc::sync::Arc;
use core::ffi::c_void;
use core::ops::DerefMut;
use core::sync::atomic::Ordering;

use crate::errno::*;
use crate::fd::{get_object, insert_object, FD_COUNTER, OBJECT_MAP};
use crate::net::{NetworkState, NIC};
use crate::syscalls::net::*;

mod tcp;
mod udp;

pub(crate) extern "C" fn __sys_socket(domain: i32, type_: i32, protocol: i32) -> i32 {
	debug!(
		"sys_socket: domain {}, type {}, protocol {}",
		domain, type_, protocol
	);

	if (domain != AF_INET && domain != AF_INET6)
		|| type_ != SOCK_STREAM
		|| (protocol != 0 && protocol != IPPROTO_UDP && protocol != IPPROTO_TCP)
	{
		-EINVAL
	} else {
		let mut guard = NIC.lock();

		if let NetworkState::Initialized(nic) = guard.deref_mut() {
			let fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);

			if protocol == IPPROTO_UDP {
				let handle = nic.create_udp_handle().unwrap();
				let socket = self::udp::Socket::new(handle);
				if OBJECT_MAP.lock().try_insert(fd, Arc::new(socket)).is_err() {
					-EINVAL
				} else {
					fd
				}
			} else {
				let handle = nic.create_tcp_handle().unwrap();
				if domain == AF_INET {
					let socket = self::tcp::Socket::<self::tcp::IPv4>::new(handle);
					if OBJECT_MAP.lock().try_insert(fd, Arc::new(socket)).is_err() {
						-EINVAL
					} else {
						fd
					}
				} else {
					let socket = self::tcp::Socket::<self::tcp::IPv6>::new(handle);
					if OBJECT_MAP.lock().try_insert(fd, Arc::new(socket)).is_err() {
						-EINVAL
					} else {
						fd
					}
				}
			}
		} else {
			-EINVAL
		}
	}
}

pub(crate) extern "C" fn __sys_accept(
	fd: i32,
	addr: *mut sockaddr,
	addrlen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| e,
		|v| {
			let result = (*v).accept(addr, addrlen);
			if result >= 0 {
				let new_obj = (*v).clone_box();
				insert_object(fd, Arc::from(new_obj));
				let new_fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);
				(*v).listen(1);
				insert_object(new_fd, v.clone());
				new_fd
			} else {
				result
			}
		},
	)
}

pub(crate) extern "C" fn __sys_listen(fd: i32, backlog: i32) -> i32 {
	if backlog > 1 {
		-EINVAL
	} else {
		let obj = get_object(fd);
		obj.map_or_else(|e| e, |v| (*v).listen(backlog))
	}
}

pub(crate) extern "C" fn __sys_bind(fd: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).bind(name, namelen))
}

pub(crate) extern "C" fn __sys_connect(fd: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).connect(name, namelen))
}

pub(crate) extern "C" fn __sys_getsockname(
	fd: i32,
	name: *mut sockaddr,
	namelen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).getsockname(name, namelen))
}

pub(crate) extern "C" fn __sys_setsockopt(
	fd: i32,
	level: i32,
	optname: i32,
	optval: *const c_void,
	optlen: socklen_t,
) -> i32 {
	debug!(
		"sys_setsockopt: {}, level {}, optname {}",
		fd, level, optname
	);

	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).setsockopt(level, optname, optval, optlen))
}

pub(crate) extern "C" fn __sys_getsockopt(
	fd: i32,
	level: i32,
	optname: i32,
	optval: *mut c_void,
	optlen: *mut socklen_t,
) -> i32 {
	debug!(
		"sys_getsockopt: {}, level {}, optname {}",
		fd, level, optname
	);

	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).getsockopt(level, optname, optval, optlen))
}

pub(crate) extern "C" fn __sys_getpeername(
	fd: i32,
	name: *mut sockaddr,
	namelen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).getpeername(name, namelen))
}

pub extern "C" fn __sys_freeaddrinfo(_ai: *mut addrinfo) {}

pub extern "C" fn __sys_getaddrinfo(
	_nodename: *const u8,
	_servname: *const u8,
	_hints: *const addrinfo,
	_res: *mut *mut addrinfo,
) -> i32 {
	-EINVAL
}

pub extern "C" fn __sys_shutdown_socket(fd: i32, how: i32) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).shutdown(how))
}

pub extern "C" fn __sys_recv(fd: i32, buf: *mut u8, len: usize) -> isize {
	let obj = get_object(fd);
	obj.map_or_else(|e| e as isize, |v| (*v).read(buf, len))
}
