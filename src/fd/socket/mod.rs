use alloc::sync::Arc;
use core::ffi::c_void;
use core::ops::DerefMut;
use core::sync::atomic::Ordering;

use crate::errno::*;
use crate::executor::network::{NetworkState, NIC};
use crate::fd::{get_object, insert_object, FD_COUNTER, OBJECT_MAP};
use crate::syscalls::net::*;

#[cfg(feature = "tcp")]
mod tcp;
#[cfg(feature = "udp")]
mod udp;

pub(crate) extern "C" fn __sys_socket(domain: i32, type_: i32, protocol: i32) -> i32 {
	debug!(
		"sys_socket: domain {}, type {}, protocol {}",
		domain, type_, protocol
	);

	if (domain != AF_INET && domain != AF_INET6)
		|| (type_ != SOCK_STREAM && type_ != SOCK_DGRAM)
		|| protocol != 0
	{
		-EINVAL
	} else {
		let mut guard = NIC.lock();

		if let NetworkState::Initialized(nic) = guard.deref_mut() {
			let fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);

			#[cfg(feature = "udp")]
			if type_ == SOCK_DGRAM {
				let handle = nic.create_udp_handle().unwrap();
				if domain == AF_INET {
					let socket = self::udp::Socket::<self::udp::IPv4>::new(handle);
					if OBJECT_MAP.write().try_insert(fd, Arc::new(socket)).is_err() {
						return -EINVAL;
					} else {
						return fd;
					}
				} else {
					let socket = self::udp::Socket::<self::udp::IPv6>::new(handle);
					if OBJECT_MAP.write().try_insert(fd, Arc::new(socket)).is_err() {
						return -EINVAL;
					} else {
						return fd;
					}
				}
			}

			#[cfg(feature = "tcp")]
			if type_ == SOCK_STREAM {
				let handle = nic.create_tcp_handle().unwrap();
				if domain == AF_INET {
					let socket = self::tcp::Socket::<self::tcp::IPv4>::new(handle);
					if OBJECT_MAP.write().try_insert(fd, Arc::new(socket)).is_err() {
						return -EINVAL;
					} else {
						return fd;
					}
				} else {
					let socket = self::tcp::Socket::<self::tcp::IPv6>::new(handle);
					if OBJECT_MAP.write().try_insert(fd, Arc::new(socket)).is_err() {
						return -EINVAL;
					} else {
						return fd;
					}
				}
			}

			-EINVAL
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
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).accept(addr, addrlen).map_or_else(
				|e| -num::ToPrimitive::to_i32(&e).unwrap(),
				|_| {
					let new_obj = dyn_clone::clone_box(&*v);
					insert_object(fd, Arc::from(new_obj));
					let new_fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);
					match (*v).listen(1) {
						Ok(_) => {
							insert_object(new_fd, v.clone());
							new_fd
						}
						Err(e) => -num::ToPrimitive::to_i32(&e).unwrap(),
					}
				},
			)
		},
	)
}

pub(crate) extern "C" fn __sys_listen(fd: i32, backlog: i32) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).listen(backlog)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
}

pub(crate) extern "C" fn __sys_bind(fd: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).bind(name, namelen)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
}

pub(crate) extern "C" fn __sys_connect(fd: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).connect(name, namelen)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |v| v)
		},
	)
}

pub(crate) extern "C" fn __sys_getsockname(
	fd: i32,
	name: *mut sockaddr,
	namelen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).getsockname(name, namelen)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
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
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).setsockopt(level, optname, optval, optlen)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
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
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).getsockopt(level, optname, optval, optlen)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
}

pub(crate) extern "C" fn __sys_getpeername(
	fd: i32,
	name: *mut sockaddr,
	namelen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).getpeername(name, namelen)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
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
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).shutdown(how)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
}

pub extern "C" fn __sys_recv(fd: i32, buf: *mut u8, len: usize) -> isize {
	let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };
	let obj = get_object(fd);
	obj.map_or_else(
		|e| e as isize,
		|v| {
			(*v).read(slice)
				.map_or_else(|e| -num::ToPrimitive::to_isize(&e).unwrap(), |v| v)
		},
	)
}

pub extern "C" fn __sys_sendto(
	fd: i32,
	buf: *const u8,
	len: usize,
	_flags: i32,
	addr: *const sockaddr,
	addr_len: socklen_t,
) -> isize {
	let slice = unsafe { core::slice::from_raw_parts(buf, len) };
	let obj = get_object(fd);
	obj.map_or_else(
		|e| e as isize,
		|v| {
			(*v).sendto(slice, addr, addr_len)
				.map_or_else(|e| -num::ToPrimitive::to_isize(&e).unwrap(), |v| v)
		},
	)
}

pub extern "C" fn __sys_recvfrom(
	fd: i32,
	buf: *mut u8,
	len: usize,
	_flags: i32,
	addr: *mut sockaddr,
	addr_len: *mut socklen_t,
) -> isize {
	let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };
	let obj = get_object(fd);
	obj.map_or_else(
		|e| e as isize,
		|v| {
			(*v).recvfrom(slice, addr, addr_len)
				.map_or_else(|e| -num::ToPrimitive::to_isize(&e).unwrap(), |v| v)
		},
	)
}
