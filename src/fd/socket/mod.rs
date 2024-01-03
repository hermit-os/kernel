use alloc::sync::Arc;
use core::ffi::c_void;
use core::mem::size_of;
use core::ops::DerefMut;
use core::sync::atomic::Ordering;

use smoltcp::wire::{IpAddress, IpEndpoint, IpListenEndpoint};

use crate::errno::*;
use crate::executor::network::{NetworkState, NIC};
use crate::fd::{get_object, insert_object, SocketOption, FD_COUNTER, OBJECT_MAP};
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
				let socket = self::udp::Socket::new(handle);
				if OBJECT_MAP.write().try_insert(fd, Arc::new(socket)).is_err() {
					return -EINVAL;
				} else {
					return fd;
				}
			}

			#[cfg(feature = "tcp")]
			if type_ == SOCK_STREAM {
				let handle = nic.create_tcp_handle().unwrap();
				let socket = self::tcp::Socket::new(handle);
				if OBJECT_MAP.write().try_insert(fd, Arc::new(socket)).is_err() {
					return -EINVAL;
				} else {
					return fd;
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
			(*v).accept().map_or_else(
				|e| -num::ToPrimitive::to_i32(&e).unwrap(),
				|endpoint| {
					let new_obj = dyn_clone::clone_box(&*v);
					insert_object(fd, Arc::from(new_obj));
					let new_fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);
					insert_object(new_fd, v.clone());

					if !addr.is_null() && !addrlen.is_null() {
						let addrlen = unsafe { &mut *addrlen };

						match endpoint.addr {
							IpAddress::Ipv4(_) => {
								if *addrlen >= size_of::<sockaddr_in>().try_into().unwrap() {
									let addr = unsafe { &mut *(addr as *mut sockaddr_in) };
									*addr = sockaddr_in::from(endpoint);
									*addrlen = size_of::<sockaddr_in>().try_into().unwrap();
								}
							}
							IpAddress::Ipv6(_) => {
								if *addrlen >= size_of::<sockaddr_in6>().try_into().unwrap() {
									let addr = unsafe { &mut *(addr as *mut sockaddr_in6) };
									*addr = sockaddr_in6::from(endpoint);
									*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();
								}
							}
						}
					}

					new_fd
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
	let endpoint = if namelen == size_of::<sockaddr_in>().try_into().unwrap() {
		IpListenEndpoint::from(unsafe { *(name as *const sockaddr_in) })
	} else if namelen == size_of::<sockaddr_in6>().try_into().unwrap() {
		IpListenEndpoint::from(unsafe { *(name as *const sockaddr_in6) })
	} else {
		return -crate::errno::EINVAL;
	};

	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).bind(endpoint)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
}

pub(crate) extern "C" fn __sys_connect(fd: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	let endpoint = if namelen == size_of::<sockaddr_in>().try_into().unwrap() {
		IpEndpoint::from(unsafe { *(name as *const sockaddr_in) })
	} else if namelen == size_of::<sockaddr_in6>().try_into().unwrap() {
		IpEndpoint::from(unsafe { *(name as *const sockaddr_in6) })
	} else {
		return -crate::errno::EINVAL;
	};

	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).connect(endpoint)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
}

pub(crate) extern "C" fn __sys_getsockname(
	fd: i32,
	addr: *mut sockaddr,
	addrlen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			if let Some(endpoint) = (*v).getsockname() {
				if !addr.is_null() && !addrlen.is_null() {
					let addrlen = unsafe { &mut *addrlen };

					match endpoint.addr {
						IpAddress::Ipv4(_) => {
							if *addrlen >= size_of::<sockaddr_in>().try_into().unwrap() {
								let addr = unsafe { &mut *(addr as *mut sockaddr_in) };
								*addr = sockaddr_in::from(endpoint);
								*addrlen = size_of::<sockaddr_in>().try_into().unwrap();
							} else {
								return -crate::errno::EINVAL;
							}
						}
						IpAddress::Ipv6(_) => {
							if *addrlen >= size_of::<sockaddr_in6>().try_into().unwrap() {
								let addr = unsafe { &mut *(addr as *mut sockaddr_in6) };
								*addr = sockaddr_in6::from(endpoint);
								*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();
							} else {
								return -crate::errno::EINVAL;
							}
						}
					}
				} else {
					return -crate::errno::EINVAL;
				}
			}

			0
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

	if level == IPPROTO_TCP
		&& optname == TCP_NODELAY
		&& optlen == size_of::<i32>().try_into().unwrap()
	{
		if optval.is_null() {
			return -crate::errno::EINVAL;
		}

		let value = unsafe { *(optval as *const i32) };
		let obj = get_object(fd);
		obj.map_or_else(
			|e| -num::ToPrimitive::to_i32(&e).unwrap(),
			|v| {
				(*v).setsockopt(SocketOption::TcpNoDelay, value != 0)
					.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
			},
		)
	} else if level == SOL_SOCKET && optname == SO_REUSEADDR {
		0
	} else {
		-crate::errno::EINVAL
	}
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

	if level == IPPROTO_TCP && optname == TCP_NODELAY {
		if optval.is_null() || optlen.is_null() {
			return -crate::errno::EINVAL;
		}

		let optval = unsafe { &mut *(optval as *mut i32) };
		let optlen = unsafe { &mut *(optlen as *mut socklen_t) };
		let obj = get_object(fd);
		obj.map_or_else(
			|e| -num::ToPrimitive::to_i32(&e).unwrap(),
			|v| {
				(*v).getsockopt(SocketOption::TcpNoDelay).map_or_else(
					|e| -num::ToPrimitive::to_i32(&e).unwrap(),
					|value| {
						if value {
							*optval = 1;
						} else {
							*optval = 0;
						}
						*optlen = core::mem::size_of::<i32>().try_into().unwrap();

						0
					},
				)
			},
		)
	} else {
		-crate::errno::EINVAL
	}
}

pub(crate) extern "C" fn __sys_getpeername(
	fd: i32,
	addr: *mut sockaddr,
	addrlen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			if let Some(endpoint) = (*v).getsockname() {
				if !addr.is_null() && !addrlen.is_null() {
					let addrlen = unsafe { &mut *addrlen };

					match endpoint.addr {
						IpAddress::Ipv4(_) => {
							if *addrlen >= size_of::<sockaddr_in>().try_into().unwrap() {
								let addr = unsafe { &mut *(addr as *mut sockaddr_in) };
								*addr = sockaddr_in::from(endpoint);
								*addrlen = size_of::<sockaddr_in>().try_into().unwrap();
							} else {
								return -crate::errno::EINVAL;
							}
						}
						IpAddress::Ipv6(_) => {
							if *addrlen >= size_of::<sockaddr_in6>().try_into().unwrap() {
								let addr = unsafe { &mut *(addr as *mut sockaddr_in6) };
								*addr = sockaddr_in6::from(endpoint);
								*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();
							} else {
								return -crate::errno::EINVAL;
							}
						}
					}
				} else {
					return -crate::errno::EINVAL;
				}
			}

			0
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
		|e| -num::ToPrimitive::to_isize(&e).unwrap(),
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
	let endpoint = if addr_len == size_of::<sockaddr_in>().try_into().unwrap() {
		IpEndpoint::from(unsafe { *(addr as *const sockaddr_in) })
	} else if addr_len == size_of::<sockaddr_in6>().try_into().unwrap() {
		IpEndpoint::from(unsafe { *(addr as *const sockaddr_in6) })
	} else {
		return (-crate::errno::EINVAL).try_into().unwrap();
	};
	let slice = unsafe { core::slice::from_raw_parts(buf, len) };
	let obj = get_object(fd);

	obj.map_or_else(
		|e| -num::ToPrimitive::to_isize(&e).unwrap(),
		|v| {
			(*v).sendto(slice, endpoint)
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
	addrlen: *mut socklen_t,
) -> isize {
	let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_isize(&e).unwrap(),
		|v| {
			(*v).recvfrom(slice).map_or_else(
				|e| -num::ToPrimitive::to_isize(&e).unwrap(),
				|(len, endpoint)| {
					if !addr.is_null() && !addrlen.is_null() {
						let addrlen = unsafe { &mut *addrlen };

						match endpoint.addr {
							IpAddress::Ipv4(_) => {
								if *addrlen >= size_of::<sockaddr_in>().try_into().unwrap() {
									let addr = unsafe { &mut *(addr as *mut sockaddr_in) };
									*addr = sockaddr_in::from(endpoint);
									*addrlen = size_of::<sockaddr_in>().try_into().unwrap();
								} else {
									return (-crate::errno::EINVAL).try_into().unwrap();
								}
							}
							IpAddress::Ipv6(_) => {
								if *addrlen >= size_of::<sockaddr_in6>().try_into().unwrap() {
									let addr = unsafe { &mut *(addr as *mut sockaddr_in6) };
									*addr = sockaddr_in6::from(endpoint);
									*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();
								} else {
									return (-crate::errno::EINVAL).try_into().unwrap();
								}
							}
						}
					}

					len
				},
			)
		},
	)
}
