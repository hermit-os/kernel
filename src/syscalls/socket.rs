#![allow(dead_code)]
#![allow(nonstandard_style)]
use alloc::sync::Arc;
use core::ffi::{c_char, c_void};
use core::mem::size_of;
#[allow(unused_imports)]
use core::ops::DerefMut;

use cfg_if::cfg_if;
#[cfg(any(feature = "tcp", feature = "udp"))]
use smoltcp::wire::{IpAddress, IpEndpoint, IpListenEndpoint};

use crate::errno::*;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::executor::network::{NIC, NetworkState};
#[cfg(feature = "tcp")]
use crate::fd::socket::tcp;
#[cfg(feature = "udp")]
use crate::fd::socket::udp;
#[cfg(feature = "vsock")]
use crate::fd::socket::vsock::{self, VsockEndpoint, VsockListenEndpoint};
use crate::fd::{
	self, Endpoint, ListenEndpoint, ObjectInterface, SocketOption, get_object, insert_object,
};
use crate::syscalls::block_on;

pub const AF_UNSPEC: i32 = 0;
pub const AF_INET_OLD: i32 = 0;
pub const AF_INET: i32 = 3;
pub const AF_INET6: i32 = 1;
pub const AF_VSOCK: i32 = 2;
pub const IPPROTO_IP: i32 = 0;
pub const IPPROTO_IPV6: i32 = 41;
pub const IPPROTO_TCP: i32 = 6;
pub const IPPROTO_UDP: i32 = 17;
pub const IPV6_ADD_MEMBERSHIP: i32 = 12;
pub const IPV6_DROP_MEMBERSHIP: i32 = 13;
pub const IPV6_MULTICAST_LOOP: i32 = 19;
pub const IPV6_V6ONLY: i32 = 27;
pub const IP_TOS: i32 = 1;
pub const IP_TTL: i32 = 2;
pub const IP_MULTICAST_TTL: i32 = 5;
pub const IP_MULTICAST_LOOP: i32 = 7;
pub const IP_ADD_MEMBERSHIP: i32 = 3;
pub const IP_DROP_MEMBERSHIP: i32 = 4;
pub const SOL_SOCKET: i32 = 4095;
pub const SO_REUSEADDR: i32 = 0x0004;
pub const SO_KEEPALIVE: i32 = 0x0008;
pub const SO_BROADCAST: i32 = 0x0020;
pub const SO_LINGER: i32 = 0x0080;
pub const SO_SNDBUF: i32 = 0x1001;
pub const SO_RCVBUF: i32 = 0x1002;
pub const SO_SNDTIMEO: i32 = 0x1005;
pub const SO_RCVTIMEO: i32 = 0x1006;
pub const SO_ERROR: i32 = 0x1007;
pub const TCP_NODELAY: i32 = 1;
pub const MSG_PEEK: i32 = 1;
pub const EAI_AGAIN: i32 = 2;
pub const EAI_BADFLAGS: i32 = 3;
pub const EAI_FAIL: i32 = 4;
pub const EAI_FAMILY: i32 = 5;
pub const EAI_MEMORY: i32 = 6;
pub const EAI_NODATA: i32 = 7;
pub const EAI_NONAME: i32 = 8;
pub const EAI_SERVICE: i32 = 9;
pub const EAI_SOCKTYPE: i32 = 10;
pub const EAI_SYSTEM: i32 = 11;
pub const EAI_OVERFLOW: i32 = 14;
pub type sa_family_t = u8;
pub type socklen_t = u32;
pub type in_addr_t = u32;
pub type in_port_t = u16;

bitflags! {
	#[derive(Debug, Copy, Clone)]
	#[repr(C)]
	pub struct SockType: i32 {
		const SOCK_DGRAM = 2;
		const SOCK_STREAM = 1;
		const SOCK_NONBLOCK = 0o4000;
		const SOCK_CLOEXEC = 0o40000;
	}
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct in_addr {
	pub s_addr: in_addr_t,
}

#[repr(C, align(4))]
#[derive(Debug, Default, Copy, Clone)]
pub struct in6_addr {
	pub s6_addr: [u8; 16],
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct sockaddr {
	pub sa_len: u8,
	pub sa_family: sa_family_t,
	pub sa_data: [c_char; 14],
}

#[cfg(feature = "vsock")]
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct sockaddr_vm {
	pub svm_len: u8,
	pub svm_family: sa_family_t,
	pub svm_reserved1: u16,
	pub svm_port: u32,
	pub svm_cid: u32,
	pub svm_zero: [u8; 4],
}

#[cfg(feature = "vsock")]
impl From<sockaddr_vm> for VsockListenEndpoint {
	fn from(addr: sockaddr_vm) -> VsockListenEndpoint {
		let port = addr.svm_port;
		let cid = if addr.svm_cid < u32::MAX {
			Some(addr.svm_cid)
		} else {
			None
		};

		VsockListenEndpoint::new(port, cid)
	}
}

#[cfg(feature = "vsock")]
impl From<sockaddr_vm> for VsockEndpoint {
	fn from(addr: sockaddr_vm) -> VsockEndpoint {
		let port = addr.svm_port;
		let cid = addr.svm_cid;

		VsockEndpoint::new(port, cid)
	}
}

#[cfg(feature = "vsock")]
impl From<VsockEndpoint> for sockaddr_vm {
	fn from(endpoint: VsockEndpoint) -> Self {
		Self {
			svm_len: core::mem::size_of::<sockaddr_vm>().try_into().unwrap(),
			svm_family: AF_VSOCK.try_into().unwrap(),
			svm_port: endpoint.port,
			svm_cid: endpoint.cid,
			..Default::default()
		}
	}
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct sockaddr_in {
	pub sin_len: u8,
	pub sin_family: sa_family_t,
	pub sin_port: in_port_t,
	pub sin_addr: in_addr,
	pub sin_zero: [c_char; 8],
}

#[cfg(any(feature = "tcp", feature = "udp"))]
impl From<sockaddr_in> for IpListenEndpoint {
	fn from(addr: sockaddr_in) -> IpListenEndpoint {
		let port = u16::from_be(addr.sin_port);
		if addr.sin_addr.s_addr == 0 {
			IpListenEndpoint { addr: None, port }
		} else {
			let s_addr = addr.sin_addr.s_addr.to_ne_bytes();

			let address = IpAddress::v4(s_addr[0], s_addr[1], s_addr[2], s_addr[3]);

			IpListenEndpoint::from((address, port))
		}
	}
}

#[cfg(any(feature = "tcp", feature = "udp"))]
impl From<sockaddr_in> for IpEndpoint {
	fn from(addr: sockaddr_in) -> IpEndpoint {
		let port = u16::from_be(addr.sin_port);
		let s_addr = addr.sin_addr.s_addr.to_ne_bytes();
		let address = IpAddress::v4(s_addr[0], s_addr[1], s_addr[2], s_addr[3]);

		IpEndpoint::from((address, port))
	}
}

#[cfg(any(feature = "tcp", feature = "udp"))]
impl From<IpEndpoint> for sockaddr_in {
	fn from(endpoint: IpEndpoint) -> Self {
		match endpoint.addr {
			IpAddress::Ipv4(ip) => {
				let sin_addr = in_addr {
					s_addr: u32::from_ne_bytes(ip.octets()),
				};

				Self {
					sin_len: core::mem::size_of::<sockaddr_in>().try_into().unwrap(),
					sin_port: endpoint.port.to_be(),
					sin_family: AF_INET.try_into().unwrap(),
					sin_addr,
					..Default::default()
				}
			}
			IpAddress::Ipv6(_) => panic!("Unable to convert IPv6 address to sockadd_in"),
		}
	}
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct sockaddr_in6 {
	pub sin6_len: u8,
	pub sin6_family: sa_family_t,
	pub sin6_port: in_port_t,
	pub sin6_flowinfo: u32,
	pub sin6_addr: in6_addr,
	pub sin6_scope_id: u32,
}

#[cfg(any(feature = "tcp", feature = "udp"))]
impl From<sockaddr_in6> for IpListenEndpoint {
	fn from(addr: sockaddr_in6) -> IpListenEndpoint {
		let port = u16::from_be(addr.sin6_port);
		if addr.sin6_addr.s6_addr.into_iter().all(|b| b == 0) {
			IpListenEndpoint { addr: None, port }
		} else {
			let s6_addr = addr.sin6_addr.s6_addr;
			let a0 = (u16::from(s6_addr[0]) << 8) | u16::from(s6_addr[1]);
			let a1 = (u16::from(s6_addr[2]) << 8) | u16::from(s6_addr[3]);
			let a2 = (u16::from(s6_addr[4]) << 8) | u16::from(s6_addr[5]);
			let a3 = (u16::from(s6_addr[6]) << 8) | u16::from(s6_addr[7]);
			let a4 = (u16::from(s6_addr[8]) << 8) | u16::from(s6_addr[9]);
			let a5 = (u16::from(s6_addr[10]) << 8) | u16::from(s6_addr[11]);
			let a6 = (u16::from(s6_addr[12]) << 8) | u16::from(s6_addr[13]);
			let a7 = (u16::from(s6_addr[14]) << 8) | u16::from(s6_addr[15]);
			let address = IpAddress::v6(a0, a1, a2, a3, a4, a5, a6, a7);

			IpListenEndpoint::from((address, port))
		}
	}
}

#[cfg(any(feature = "tcp", feature = "udp"))]
impl From<sockaddr_in6> for IpEndpoint {
	fn from(addr: sockaddr_in6) -> IpEndpoint {
		let port = u16::from_be(addr.sin6_port);
		let s6_addr = addr.sin6_addr.s6_addr;
		let a0 = (u16::from(s6_addr[0]) << 8) | u16::from(s6_addr[1]);
		let a1 = (u16::from(s6_addr[2]) << 8) | u16::from(s6_addr[3]);
		let a2 = (u16::from(s6_addr[4]) << 8) | u16::from(s6_addr[5]);
		let a3 = (u16::from(s6_addr[6]) << 8) | u16::from(s6_addr[7]);
		let a4 = (u16::from(s6_addr[8]) << 8) | u16::from(s6_addr[9]);
		let a5 = (u16::from(s6_addr[10]) << 8) | u16::from(s6_addr[11]);
		let a6 = (u16::from(s6_addr[12]) << 8) | u16::from(s6_addr[13]);
		let a7 = (u16::from(s6_addr[14]) << 8) | u16::from(s6_addr[15]);
		let address = IpAddress::v6(a0, a1, a2, a3, a4, a5, a6, a7);

		IpEndpoint::from((address, port))
	}
}

#[cfg(any(feature = "tcp", feature = "udp"))]
impl From<IpEndpoint> for sockaddr_in6 {
	fn from(endpoint: IpEndpoint) -> Self {
		match endpoint.addr {
			IpAddress::Ipv6(ip) => {
				let mut in6_addr = in6_addr::default();
				in6_addr.s6_addr.copy_from_slice(&ip.octets());

				Self {
					sin6_len: core::mem::size_of::<sockaddr_in6>().try_into().unwrap(),
					sin6_port: endpoint.port.to_be(),
					sin6_family: AF_INET6.try_into().unwrap(),
					sin6_addr: in6_addr,
					..Default::default()
				}
			}
			IpAddress::Ipv4(_) => panic!("Unable to convert IPv4 address to sockadd_in6"),
		}
	}
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
	pub ai_canonname: *mut c_char,
	pub ai_addr: *mut sockaddr,
	pub ai_next: *mut addrinfo,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct linger {
	pub l_onoff: i32,
	pub l_linger: i32,
}

#[cfg(not(feature = "dns"))]
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getaddrbyname(
	_name: *const c_char,
	_inaddr: *mut u8,
	_len: usize,
) -> i32 {
	error!("Please enable the feature 'dns' to determine the network ip by name.");
	-ENOSYS
}

/// The system call `sys_getaddrbyname` determine the network host entry.
/// It expects an array of u8 with a size of in_addr or of in6_addr.
/// The result of the DNS request will be stored in this array.
///
/// # Example
///
/// ```
/// use hermit_abi::in_addr;
/// let c_string = std::ffi::CString::new("rust-lang.org").expect("CString::new failed");
/// let name = c_string.into_raw();
/// let mut inaddr: in_addr = Default::default();
/// let _ = unsafe {
///         hermit_abi::getaddrbyname(
///                 name,
///                 &mut inaddr as *mut _ as *mut u8,
///                 std::mem::size_of::<in_addr>(),
///         )
/// };
///
/// // retake pointer to free memory
/// let _ = CString::from_raw(name);
/// ```
#[cfg(feature = "dns")]
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getaddrbyname(
	name: *const c_char,
	inaddr: *mut u8,
	len: usize,
) -> i32 {
	use alloc::borrow::ToOwned;

	use smoltcp::wire::DnsQueryType;

	use crate::executor::block_on;
	use crate::executor::network::get_query_result;

	if len != size_of::<in_addr>() && len != size_of::<in6_addr>() {
		return -EINVAL;
	}

	if inaddr.is_null() {
		return -EINVAL;
	}

	let query_type = if len == size_of::<in6_addr>() {
		DnsQueryType::Aaaa
	} else {
		DnsQueryType::A
	};

	let name = unsafe { core::ffi::CStr::from_ptr(name) };
	let name = if let Ok(name) = name.to_str() {
		name.to_owned()
	} else {
		return -EINVAL;
	};

	let query = {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let query = nic.start_query(&name, query_type).unwrap();
		nic.poll_common(crate::executor::network::now());

		query
	};

	match block_on(get_query_result(query), None) {
		Ok(addr_vec) => {
			let slice = unsafe { core::slice::from_raw_parts_mut(inaddr, len) };

			match addr_vec[0] {
				IpAddress::Ipv4(ipv4_addr) => slice.copy_from_slice(&ipv4_addr.octets()),
				IpAddress::Ipv6(ipv6_addr) => slice.copy_from_slice(&ipv6_addr.octets()),
			}

			0
		}
		Err(e) => -i32::from(e),
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_socket(domain: i32, type_: SockType, protocol: i32) -> i32 {
	debug!("sys_socket: domain {domain}, type {type_:?}, protocol {protocol}");

	if protocol != 0 {
		return -EINVAL;
	}

	#[cfg(feature = "vsock")]
	if domain == AF_VSOCK && type_.intersects(SockType::SOCK_STREAM) {
		let socket = Arc::new(async_lock::RwLock::new(vsock::Socket::new()));

		if type_.contains(SockType::SOCK_NONBLOCK) {
			block_on(socket.set_status_flags(fd::StatusFlags::O_NONBLOCK), None).unwrap();
		}

		let fd = insert_object(socket).expect("FD is already used");

		return fd;
	}

	#[cfg(any(feature = "tcp", feature = "udp"))]
	if (domain == AF_INET_OLD || domain == AF_INET || domain == AF_INET6)
		&& type_.intersects(SockType::SOCK_STREAM | SockType::SOCK_DGRAM)
	{
		let mut guard = NIC.lock();

		if let NetworkState::Initialized(nic) = &mut *guard {
			#[cfg(feature = "udp")]
			if type_.contains(SockType::SOCK_DGRAM) {
				let handle = nic.create_udp_handle().unwrap();
				drop(guard);
				let socket = Arc::new(async_lock::RwLock::new(udp::Socket::new(handle, domain)));

				if type_.contains(SockType::SOCK_NONBLOCK) {
					block_on(socket.set_status_flags(fd::StatusFlags::O_NONBLOCK), None).unwrap();
				}

				let fd = insert_object(socket).expect("FD is already used");

				return fd;
			}

			#[cfg(feature = "tcp")]
			if type_.contains(SockType::SOCK_STREAM) {
				let handle = nic.create_tcp_handle().unwrap();
				drop(guard);
				let socket = Arc::new(async_lock::RwLock::new(tcp::Socket::new(handle, domain)));

				if type_.contains(SockType::SOCK_NONBLOCK) {
					block_on(socket.set_status_flags(fd::StatusFlags::O_NONBLOCK), None).unwrap();
				}

				let fd = insert_object(socket).expect("FD is already used");

				return fd;
			}
		}
	}

	-EINVAL
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_accept(fd: i32, addr: *mut sockaddr, addrlen: *mut socklen_t) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -i32::from(e),
		|v| {
			block_on((*v).accept(), None).map_or_else(
				|e| -i32::from(e),
				|(obj, endpoint)| match endpoint {
					#[cfg(any(feature = "tcp", feature = "udp"))]
					Endpoint::Ip(endpoint) => {
						let new_fd = insert_object(obj).unwrap();

						if !addr.is_null() && !addrlen.is_null() {
							let addrlen = unsafe { &mut *addrlen };

							match endpoint.addr {
								IpAddress::Ipv4(_) => {
									if *addrlen >= u32::try_from(size_of::<sockaddr_in>()).unwrap()
									{
										let addr = unsafe { &mut *addr.cast() };
										*addr = sockaddr_in::from(endpoint);
										addr.sin_family = block_on(v.inet_domain(), None)
											.unwrap()
											.try_into()
											.unwrap();
										*addrlen = size_of::<sockaddr_in>().try_into().unwrap();
									}
								}
								IpAddress::Ipv6(_) => {
									if *addrlen >= u32::try_from(size_of::<sockaddr_in6>()).unwrap()
									{
										let addr = unsafe { &mut *addr.cast() };
										*addr = sockaddr_in6::from(endpoint);
										*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();
									}
								}
							}
						}

						new_fd
					}
					#[cfg(feature = "vsock")]
					Endpoint::Vsock(endpoint) => {
						let new_fd = insert_object(v.clone()).unwrap();

						if !addr.is_null() && !addrlen.is_null() {
							let addrlen = unsafe { &mut *addrlen };

							if *addrlen >= u32::try_from(size_of::<sockaddr_vm>()).unwrap() {
								let addr = unsafe { &mut *addr.cast() };
								*addr = sockaddr_vm::from(endpoint);
								*addrlen = size_of::<sockaddr_vm>().try_into().unwrap();
							}
						}

						new_fd
					}
				},
			)
		},
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_listen(fd: i32, backlog: i32) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -i32::from(e),
		|v| block_on((*v).listen(backlog), None).map_or_else(|e| -i32::from(e), |()| 0),
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_bind(fd: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	if name.is_null() {
		return -crate::errno::EINVAL;
	}

	let family: i32 = unsafe { (*name).sa_family.into() };

	let obj = get_object(fd);
	obj.map_or_else(
		|e| -i32::from(e),
		|v| match family {
			#[cfg(any(feature = "tcp", feature = "udp"))]
			AF_INET_OLD | AF_INET => {
				if namelen < u32::try_from(size_of::<sockaddr_in>()).unwrap() {
					return -crate::errno::EINVAL;
				}
				let endpoint = IpListenEndpoint::from(unsafe { *name.cast::<sockaddr_in>() });
				block_on((*v).bind(ListenEndpoint::Ip(endpoint)), None)
					.map_or_else(|e| -i32::from(e), |()| 0)
			}
			#[cfg(any(feature = "tcp", feature = "udp"))]
			AF_INET6 => {
				if namelen < u32::try_from(size_of::<sockaddr_in6>()).unwrap() {
					return -crate::errno::EINVAL;
				}
				let endpoint = IpListenEndpoint::from(unsafe { *name.cast::<sockaddr_in6>() });
				block_on((*v).bind(ListenEndpoint::Ip(endpoint)), None)
					.map_or_else(|e| -i32::from(e), |()| 0)
			}
			#[cfg(feature = "vsock")]
			AF_VSOCK => {
				if namelen < u32::try_from(size_of::<sockaddr_vm>()).unwrap() {
					return -crate::errno::EINVAL;
				}
				let endpoint = VsockListenEndpoint::from(unsafe { *name.cast::<sockaddr_vm>() });
				block_on((*v).bind(ListenEndpoint::Vsock(endpoint)), None)
					.map_or_else(|e| -i32::from(e), |()| 0)
			}
			_ => -crate::errno::EINVAL,
		},
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_connect(fd: i32, name: *const sockaddr, namelen: socklen_t) -> i32 {
	if name.is_null() {
		return -crate::errno::EINVAL;
	}

	let sa_family = unsafe { i32::from((*name).sa_family) };

	let endpoint = match sa_family {
		#[cfg(any(feature = "tcp", feature = "udp"))]
		AF_INET_OLD | AF_INET => {
			if namelen < u32::try_from(size_of::<sockaddr_in>()).unwrap() {
				return -crate::errno::EINVAL;
			}
			Endpoint::Ip(IpEndpoint::from(unsafe { *name.cast::<sockaddr_in>() }))
		}
		#[cfg(any(feature = "tcp", feature = "udp"))]
		AF_INET6 => {
			if namelen < u32::try_from(size_of::<sockaddr_in6>()).unwrap() {
				return -crate::errno::EINVAL;
			}
			Endpoint::Ip(IpEndpoint::from(unsafe { *name.cast::<sockaddr_in6>() }))
		}
		#[cfg(feature = "vsock")]
		AF_VSOCK => {
			if namelen < u32::try_from(size_of::<sockaddr_vm>()).unwrap() {
				return -crate::errno::EINVAL;
			}
			Endpoint::Vsock(VsockEndpoint::from(unsafe { *name.cast::<sockaddr_vm>() }))
		}
		_ => {
			return -crate::errno::EINVAL;
		}
	};

	let obj = get_object(fd);
	obj.map_or_else(
		|e| -i32::from(e),
		|v| block_on((*v).connect(endpoint), None).map_or_else(|e| -i32::from(e), |()| 0),
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getsockname(
	fd: i32,
	addr: *mut sockaddr,
	addrlen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -i32::from(e),
		|v| {
			if let Ok(Some(endpoint)) = block_on((*v).getsockname(), None) {
				if !addr.is_null() && !addrlen.is_null() {
					let addrlen = unsafe { &mut *addrlen };

					match endpoint {
						#[cfg(any(feature = "tcp", feature = "udp"))]
						Endpoint::Ip(endpoint) => match endpoint.addr {
							IpAddress::Ipv4(_) => {
								if *addrlen >= u32::try_from(size_of::<sockaddr_in>()).unwrap() {
									let addr = unsafe { &mut *addr.cast() };
									*addr = sockaddr_in::from(endpoint);
									addr.sin_family = block_on(v.inet_domain(), None)
										.unwrap()
										.try_into()
										.unwrap();
									*addrlen = size_of::<sockaddr_in>().try_into().unwrap();

									0
								} else {
									-crate::errno::EINVAL
								}
							}
							#[cfg(any(feature = "tcp", feature = "udp"))]
							IpAddress::Ipv6(_) => {
								if *addrlen >= u32::try_from(size_of::<sockaddr_in6>()).unwrap() {
									let addr = unsafe { &mut *addr.cast() };
									*addr = sockaddr_in6::from(endpoint);
									*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();

									0
								} else {
									-crate::errno::EINVAL
								}
							}
						},
						#[cfg(feature = "vsock")]
						Endpoint::Vsock(_) => {
							if *addrlen >= u32::try_from(size_of::<sockaddr_vm>()).unwrap() {
								warn!("unsupported device");
								0
							} else {
								-crate::errno::EINVAL
							}
						}
					}
				} else {
					-crate::errno::EINVAL
				}
			} else {
				-crate::errno::EINVAL
			}
		},
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_setsockopt(
	fd: i32,
	level: i32,
	optname: i32,
	optval: *const c_void,
	optlen: socklen_t,
) -> i32 {
	debug!("sys_setsockopt: {fd}, level {level}, optname {optname}");

	if level == IPPROTO_TCP
		&& optname == TCP_NODELAY
		&& optlen == u32::try_from(size_of::<i32>()).unwrap()
	{
		if optval.is_null() {
			return -crate::errno::EINVAL;
		}

		let value = unsafe { *optval.cast::<i32>() };
		let obj = get_object(fd);
		obj.map_or_else(
			|e| -i32::from(e),
			|v| {
				block_on((*v).setsockopt(SocketOption::TcpNoDelay, value != 0), None)
					.map_or_else(|e| -i32::from(e), |()| 0)
			},
		)
	} else if level == SOL_SOCKET && optname == SO_REUSEADDR {
		0
	} else {
		-crate::errno::EINVAL
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getsockopt(
	fd: i32,
	level: i32,
	optname: i32,
	optval: *mut c_void,
	optlen: *mut socklen_t,
) -> i32 {
	debug!("sys_getsockopt: {fd}, level {level}, optname {optname}");

	if level == IPPROTO_TCP && optname == TCP_NODELAY {
		if optval.is_null() || optlen.is_null() {
			return -crate::errno::EINVAL;
		}

		let optval = unsafe { &mut *optval.cast::<i32>() };
		let optlen = unsafe { &mut *optlen };
		let obj = get_object(fd);
		obj.map_or_else(
			|e| -i32::from(e),
			|v| {
				block_on((*v).getsockopt(SocketOption::TcpNoDelay), None).map_or_else(
					|e| -i32::from(e),
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

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getpeername(
	fd: i32,
	addr: *mut sockaddr,
	addrlen: *mut socklen_t,
) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -i32::from(e),
		|v| {
			if let Ok(Some(endpoint)) = block_on((*v).getpeername(), None) {
				if !addr.is_null() && !addrlen.is_null() {
					let addrlen = unsafe { &mut *addrlen };

					match endpoint {
						#[cfg(any(feature = "tcp", feature = "udp"))]
						Endpoint::Ip(endpoint) => match endpoint.addr {
							IpAddress::Ipv4(_) => {
								if *addrlen >= u32::try_from(size_of::<sockaddr_in>()).unwrap() {
									let addr = unsafe { &mut *addr.cast() };
									*addr = sockaddr_in::from(endpoint);
									addr.sin_family = block_on(v.inet_domain(), None)
										.unwrap()
										.try_into()
										.unwrap();
									*addrlen = size_of::<sockaddr_in>().try_into().unwrap();
								} else {
									return -crate::errno::EINVAL;
								}
							}
							IpAddress::Ipv6(_) => {
								if *addrlen >= u32::try_from(size_of::<sockaddr_in6>()).unwrap() {
									let addr = unsafe { &mut *addr.cast() };
									*addr = sockaddr_in6::from(endpoint);
									*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();
								} else {
									return -crate::errno::EINVAL;
								}
							}
						},
						#[cfg(feature = "vsock")]
						Endpoint::Vsock(_) => {
							if *addrlen >= u32::try_from(size_of::<sockaddr_vm>()).unwrap() {
								warn!("unsupported device");
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

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_freeaddrinfo(_ai: *mut addrinfo) {}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getaddrinfo(
	_nodename: *const c_char,
	_servname: *const c_char,
	_hints: *const addrinfo,
	_res: *mut *mut addrinfo,
) -> i32 {
	-EINVAL
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_send(s: i32, mem: *const c_void, len: usize, _flags: i32) -> isize {
	unsafe { super::write(s, mem.cast(), len) }
}

fn shutdown(sockfd: i32, how: i32) -> i32 {
	let obj = get_object(sockfd);
	obj.map_or_else(
		|e| -i32::from(e),
		|v| block_on((*v).shutdown(how), None).map_or_else(|e| -i32::from(e), |()| 0),
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_shutdown(sockfd: i32, how: i32) -> i32 {
	shutdown(sockfd, how)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_shutdown_socket(fd: i32, how: i32) -> i32 {
	shutdown(fd, how)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize {
	if flags == 0 {
		let slice = unsafe { core::slice::from_raw_parts_mut(buf.cast(), len) };
		crate::fd::read(fd, slice).map_or_else(
			|e| isize::try_from(-i32::from(e)).unwrap(),
			|v| v.try_into().unwrap(),
		)
	} else {
		(-crate::errno::EINVAL).try_into().unwrap()
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_sendto(
	fd: i32,
	buf: *const u8,
	len: usize,
	_flags: i32,
	addr: *const sockaddr,
	addr_len: socklen_t,
) -> isize {
	let endpoint;

	if addr.is_null() || addr_len == 0 {
		return (-crate::errno::EINVAL).try_into().unwrap();
	}

	cfg_if! {
		if #[cfg(any(feature = "tcp", feature = "udp"))] {
			let sa_family = unsafe { i32::from((*addr).sa_family) };

			if sa_family == AF_INET_OLD || sa_family == AF_INET {
				if addr_len < u32::try_from(size_of::<sockaddr_in>()).unwrap() {
					return (-crate::errno::EINVAL).try_into().unwrap();
				}

				endpoint = Some(Endpoint::Ip(IpEndpoint::from(unsafe {*(addr.cast::<sockaddr_in>())})));
			} else if sa_family == AF_INET6 {
				if addr_len < u32::try_from(size_of::<sockaddr_in6>()).unwrap() {
					return (-crate::errno::EINVAL).try_into().unwrap();
				}

				endpoint = Some(Endpoint::Ip(IpEndpoint::from(unsafe { *(addr.cast::<sockaddr_in6>()) })));
			} else {
				endpoint = None;
			}
		} else {
			endpoint = None;
		}
	}

	if let Some(endpoint) = endpoint {
		let slice = unsafe { core::slice::from_raw_parts(buf, len) };
		let obj = get_object(fd);

		obj.map_or_else(
			|e| isize::try_from(-i32::from(e)).unwrap(),
			|v| {
				block_on((*v).sendto(slice, endpoint), None).map_or_else(
					|e| isize::try_from(-i32::from(e)).unwrap(),
					|v| v.try_into().unwrap(),
				)
			},
		)
	} else {
		(-crate::errno::EINVAL).try_into().unwrap()
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_recvfrom(
	fd: i32,
	buf: *mut u8,
	len: usize,
	_flags: i32,
	addr: *mut sockaddr,
	addrlen: *mut socklen_t,
) -> isize {
	let slice = unsafe { core::slice::from_raw_parts_mut(buf.cast(), len) };
	let obj = get_object(fd);
	obj.map_or_else(
		|e| isize::try_from(-i32::from(e)).unwrap(),
		|v| {
			block_on((*v).recvfrom(slice), None).map_or_else(
				|e| isize::try_from(-i32::from(e)).unwrap(),
				|(len, endpoint)| {
					if !addr.is_null() && !addrlen.is_null() {
						#[allow(unused_variables)]
						let addrlen = unsafe { &mut *addrlen };

						match endpoint {
							#[cfg(any(feature = "tcp", feature = "udp"))]
							Endpoint::Ip(endpoint) => match endpoint.addr {
								IpAddress::Ipv4(_) => {
									if *addrlen >= u32::try_from(size_of::<sockaddr_in>()).unwrap()
									{
										let addr = unsafe { &mut *addr.cast() };
										*addr = sockaddr_in::from(endpoint);
										addr.sin_family = block_on(v.inet_domain(), None)
											.unwrap()
											.try_into()
											.unwrap();
										*addrlen = size_of::<sockaddr_in>().try_into().unwrap();
									} else {
										return (-crate::errno::EINVAL).try_into().unwrap();
									}
								}
								IpAddress::Ipv6(_) => {
									if *addrlen >= u32::try_from(size_of::<sockaddr_in6>()).unwrap()
									{
										let addr = unsafe { &mut *addr.cast() };
										*addr = sockaddr_in6::from(endpoint);
										*addrlen = size_of::<sockaddr_in6>().try_into().unwrap();
									} else {
										return (-crate::errno::EINVAL).try_into().unwrap();
									}
								}
							},
							#[cfg(feature = "vsock")]
							_ => {
								return (-crate::errno::EINVAL).try_into().unwrap();
							}
						}
					}

					len.try_into().unwrap()
				},
			)
		},
	)
}
