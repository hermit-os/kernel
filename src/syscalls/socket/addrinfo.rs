use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};
use core::mem::MaybeUninit;
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use core::str::FromStr;
use core::{fmt, ptr};

use num_enum::{IntoPrimitive, TryFromPrimitive, TryFromPrimitiveError};

use super::{Af, Ipproto, Sock, SockFlags, sockaddr, sockaddrBox, sockaddrRef, socklen_t};

#[repr(C)]
#[derive(Default)]
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

impl Drop for addrinfo {
	fn drop(&mut self) {
		if !self.ai_addr.is_null() {
			let ai_addr = unsafe { sockaddr::as_box(self.ai_addr).unwrap() };
			drop(ai_addr);
		}

		if !self.ai_canonname.is_null() {
			let ai_canonname = unsafe { CString::from_raw(self.ai_canonname) };
			drop(ai_canonname);
		}
	}
}

#[derive(Default)]
#[repr(transparent)]
struct addrinfoList(Option<Box<addrinfo>>);

impl addrinfoList {
	fn is_empty(&self) -> bool {
		self.0.is_none()
	}

	fn iter(&self) -> addrinfoIter<'_> {
		addrinfoIter(self.0.as_deref())
	}
}

impl fmt::Debug for addrinfoList {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.iter()).finish()
	}
}

impl Extend<addrinfo> for addrinfoList {
	fn extend<T: IntoIterator<Item = addrinfo>>(&mut self, iter: T) {
		let mut place = &mut self.0;

		while let Some(some) = place {
			place = &mut some.ai_next;
		}

		for addrinfo in iter {
			assert!(addrinfo.ai_next.is_none());

			let addrinfo = place.insert(Box::new(addrinfo));
			place = &mut addrinfo.ai_next;
		}
	}
}

impl FromIterator<addrinfo> for addrinfoList {
	fn from_iter<T: IntoIterator<Item = addrinfo>>(iter: T) -> Self {
		let mut res = Self::default();
		res.extend(iter);
		res
	}
}

struct addrinfoIter<'a>(Option<&'a addrinfo>);

impl<'a> Iterator for addrinfoIter<'a> {
	type Item = &'a addrinfo;

	fn next(&mut self) -> Option<Self::Item> {
		let next = self.0?;
		self.0 = next.ai_next.as_deref();
		Some(next)
	}
}

impl<'a> IntoIterator for &'a addrinfoList {
	type Item = &'a addrinfo;

	type IntoIter = addrinfoIter<'a>;

	fn into_iter(self) -> Self::IntoIter {
		self.iter()
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
	nodename: *const c_char,
	servname: *const c_char,
	hints: *const addrinfo,
	res: &mut MaybeUninit<addrinfoList>,
) -> i32 {
	macro_rules! to_str {
		($expr:expr $(,)?) => {{
			if $expr.is_null() {
				None
			} else {
				let cstr = unsafe { CStr::from_ptr($expr) };
				match cstr.to_str() {
					Ok(s) => Some(s),
					Err(_) => return i32::from(Eai::Noname),
				}
			}
		}};
	}

	let nodename = to_str!(nodename);
	let servname = to_str!(servname);
	let hints = if hints.is_null() {
		&addrinfo::default()
	} else {
		unsafe { &*hints }
	};

	debug!("sys_getaddrinfo:");
	debug!("nodename = {nodename:?}");
	debug!("servname = {servname:?}");
	debug!("hints = {hints:?}");

	if nodename.is_none() && servname.is_none() {
		return Eai::Noname.into();
	}

	let Some(ai_family) = hints.ai_family() else {
		return Eai::Family.into();
	};

	let mut sock = None;

	if hints.ai_socktype != 0 {
		let Some((s, sock_flags)) = hints.ai_socktype() else {
			return Eai::Socktype.into();
		};

		if !sock_flags.is_empty() {
			return Eai::Socktype.into();
		};

		sock = Some(s);
	}

	let Some(proto) = hints.ai_protocol() else {
		return Eai::Socktype.into();
	};

	let services = match getaddrinfo_serv(servname, proto, sock, hints.ai_flags) {
		Ok(services) => services,
		Err(eai) => return eai.into(),
	};

	let addrs = match getaddrinfo_node(nodename, hints.ai_flags, ai_family) {
		Ok(addrs) => addrs,
		Err(eai) => return eai.into(),
	};

	let addrinfo = addrs
		.iter()
		.copied()
		.flat_map(|addr| services.iter().copied().map(move |service| (addr, service)))
		.map(|(addr, service)| {
			let addr = SocketAddr::from((addr, service.port));
			let ai_addr = sockaddrBox::from(addr);
			addrinfo {
				ai_flags: Ai::empty(),
				ai_family: u8::from(Af::from(addr.ip())).into(),
				ai_socktype: u8::from(service.socktype).into(),
				ai_protocol: u8::from(service.proto).into(),
				ai_addrlen: ai_addr.as_ref().addrlen().into(),
				ai_addr: ai_addr.into_raw(),
				ai_canonname: ptr::null_mut(),
				ai_next: None,
			}
		})
		.collect::<addrinfoList>();

	debug!("res = {addrinfo:?}");

	if addrinfo.is_empty() {
		return Eai::Noname.into();
	}

	res.write(addrinfo);

	0
}

#[derive(Clone, Copy, Debug)]
struct Service {
	port: u16,
	proto: Ipproto,
	socktype: Sock,
}

fn getaddrinfo_serv(
	name: Option<&str>,
	proto: Ipproto,
	sock: Option<Sock>,
	flags: Ai,
) -> Result<Vec<Service>, Eai> {
	let proto =
		match (sock, proto) {
			(Some(Sock::Stream), Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Tcp)
			| (None, Ipproto::Tcp) => Ipproto::Tcp,
			(Some(Sock::Dgram), Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Udp)
			| (None, Ipproto::Udp) => Ipproto::Udp,
			(Some(_), _) => return Err(Eai::Service),
			(None, proto @ (Ipproto::Ip | Ipproto::Ipv6)) => proto,
		};

	let Some(servname) = name else {
		let mut services = vec![];

		if matches!(proto, Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Tcp) {
			services.push(Service {
				port: 0,
				proto: Ipproto::Tcp,
				socktype: Sock::Stream,
			});
		}

		if matches!(proto, Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Udp) {
			services.push(Service {
				port: 0,
				proto: Ipproto::Udp,
				socktype: Sock::Dgram,
			});
		}

		return Ok(services);
	};

	if let Ok(port) = u16::from_str(servname) {
		let mut services = vec![];

		if matches!(proto, Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Tcp) {
			services.push(Service {
				port,
				proto: Ipproto::Tcp,
				socktype: Sock::Stream,
			});
		}

		if matches!(proto, Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Udp) {
			services.push(Service {
				port,
				proto: Ipproto::Udp,
				socktype: Sock::Dgram,
			});
		}

		return Ok(services);
	}

	if flags.contains(Ai::NUMERICSERV) {
		return Err(Eai::Noname);
	}

	// TODO(mkroening): add proper database
	let servname_options = match servname {
		"http" => [Service {
			port: 80,
			proto: Ipproto::Tcp,
			socktype: Sock::Stream,
		}]
		.as_slice(),
		"https" => [
			Service {
				port: 443,
				proto: Ipproto::Tcp,
				socktype: Sock::Stream,
			},
			Service {
				port: 443,
				proto: Ipproto::Udp,
				socktype: Sock::Dgram,
			},
		]
		.as_slice(),
		servname => {
			error!("Unknown service name: {servname}");
			return Err(Eai::Service);
		}
	};

	let services = servname_options
		.iter()
		.copied()
		.filter(|service| {
			matches!(
				(service.proto, proto),
				(Ipproto::Tcp, Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Tcp)
					| (Ipproto::Udp, Ipproto::Ip | Ipproto::Ipv6 | Ipproto::Udp)
			)
		})
		.collect::<Vec<_>>();

	if services.is_empty() {
		return Err(Eai::Service);
	}

	Ok(services)
}

fn getaddrinfo_node(
	nodename: Option<&str>,
	ai_flags: Ai,
	ai_family: Af,
) -> Result<Vec<IpAddr>, Eai> {
	let (mut want_ipv4, mut want_ipv6) = match ai_family {
		Af::Unspec => (true, true),
		Af::Inet => (true, false),
		Af::Inet6 => (false, true),
		Af::Unix => {
			error!("getaddrinfo_node({ai_family:?}) not implemented");
			crate::errno::ToErrno::set_errno(-i32::from(crate::errno::Errno::Nosys));
			return Err(Eai::System);
		}
		#[cfg(feature = "virtio-vsock")]
		Af::Vsock => {
			error!("getaddrinfo_node({ai_family:?}) not implemented");
			crate::errno::ToErrno::set_errno(-i32::from(crate::errno::Errno::Nosys));
			return Err(Eai::System);
		}
	};

	if ai_flags.contains(Ai::ADDRCONFIG) {
		if want_ipv4 {
			// Currently, Hermit always has an IPv4 address
			want_ipv4 = true;
		}
		if want_ipv6 {
			// Currently, Hermit never has an IPv4 address
			want_ipv6 = false;
			error!("getaddrinfo(AI_ADDRCONFIG) was called wanting an IPv6 address");
		}
	}

	let Some(nodename) = nodename else {
		let (ipv4, ipv6) = if ai_flags.contains(Ai::PASSIVE) {
			let ipv4 = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
			let ipv6 = IpAddr::V6(Ipv6Addr::UNSPECIFIED);
			(ipv4, ipv6)
		} else {
			let ipv4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
			let ipv6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
			(ipv4, ipv6)
		};

		let ip_addrs = match (want_ipv4, want_ipv6) {
			(true, true) => vec![ipv4, ipv6],
			(true, false) => vec![ipv4],
			(false, true) => vec![ipv6],
			(false, false) => vec![],
		};

		return Ok(ip_addrs);
	};

	if let Ok(addr) = IpAddr::from_str(nodename) {
		if addr.is_ipv4() && want_ipv4 || addr.is_ipv6() && want_ipv6 {
			return Ok(vec![addr]);
		} else {
			return Err(Eai::Noname);
		}
	}

	if ai_flags.contains(Ai::NUMERICHOST) {
		return Err(Eai::Noname);
	}

	cfg_if::cfg_if! {
		if #[cfg(feature = "dns")] {
			resolve(nodename, ai_flags, ai_family, want_ipv4, want_ipv6)
		} else {
			error!("Cannot resolve {nodename} as DNS is configured out");
			Err(Eai::Noname)
		}
	}
}

#[cfg(feature = "dns")]
fn resolve(
	nodename: &str,
	ai_flags: Ai,
	ai_family: Af,
	want_ipv4: bool,
	want_ipv6: bool,
) -> Result<Vec<IpAddr>, Eai> {
	use smoltcp::wire::DnsQueryType;

	use crate::errno::ToErrno;
	use crate::executor::block_on;
	use crate::executor::network::{self, NIC, get_query_result};

	macro_rules! try_io {
		($expr:expr $(,)?) => {
			match $expr {
				Ok(val) => val,
				Err(err) => {
					i32::from(err).set_errno();
					return Err(Eai::System);
				}
			}
		};
	}

	let query = |name: &str, query: DnsQueryType| {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let query = nic.start_query(name, query).unwrap();
		nic.poll_common(network::now());
		query
	};

	let ipv6_query = want_ipv6.then(|| query(nodename, DnsQueryType::Aaaa));
	let ipv6_results = ipv6_query.map(|query| block_on(get_query_result(query), None));
	let ipv6_results = try_io!(ipv6_results.transpose()).unwrap_or_default();
	let mut ipv6_results = ipv6_results
		.into_iter()
		.map(IpAddr::from)
		.collect::<Vec<_>>();

	let ipv6_mapped = ai_flags.contains(Ai::V4MAPPED)
		&& ai_family == Af::Inet6
		&& (ipv6_results.is_empty() || ai_flags.contains(Ai::ALL));

	let ipv4_query = (want_ipv4 || ipv6_mapped).then(|| query(nodename, DnsQueryType::A));
	let ipv4_results = ipv4_query.map(|query| block_on(get_query_result(query), None));
	let ipv4_results = try_io!(ipv4_results.transpose()).unwrap_or_default();
	let mut ipv4_results = ipv4_results
		.into_iter()
		.map(IpAddr::from)
		.collect::<Vec<_>>();

	if ipv6_mapped {
		for addr in &mut ipv4_results {
			let IpAddr::V4(ipv4_addr) = addr else {
				unreachable!()
			};

			*addr = IpAddr::V6(ipv4_addr.to_ipv6_mapped());
		}
	}

	ipv4_results.append(&mut ipv6_results);
	Ok(ipv4_results)
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_freeaddrinfo(ai: Option<Box<addrinfo>>) {
	drop(ai);
}

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
