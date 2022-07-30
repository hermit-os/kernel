use crate::net::executor::block_on;
use crate::net::{AsyncSocket, Handle};
use crate::DEFAULT_KEEP_ALIVE_INTERVAL;

use smoltcp::socket::TcpSocket;
use smoltcp::time::Duration;
use smoltcp::wire::IpAddress;

#[no_mangle]
pub fn sys_tcp_stream_connect(ip: &[u8], port: u16, timeout: Option<u64>) -> Result<Handle, ()> {
	let socket = AsyncSocket::new();
	block_on(socket.connect(ip, port), timeout.map(Duration::from_millis))?.map_err(|_| ())
}

#[no_mangle]
pub fn sys_tcp_stream_read(handle: Handle, buffer: &mut [u8]) -> Result<usize, ()> {
	let socket = AsyncSocket::from(handle);
	block_on(socket.read(buffer), None)?.map_err(|_| ())
}

#[no_mangle]
pub fn sys_tcp_stream_write(handle: Handle, buffer: &[u8]) -> Result<usize, ()> {
	let socket = AsyncSocket::from(handle);
	block_on(socket.write(buffer), None)?.map_err(|_| ())
}

#[no_mangle]
pub fn sys_tcp_stream_close(handle: Handle) -> Result<(), ()> {
	let socket = AsyncSocket::from(handle);
	block_on(socket.close(), None)?.map_err(|_| ())
}

//ToDo: an enum, or at least constants would be better
#[no_mangle]
pub fn sys_tcp_stream_shutdown(handle: Handle, how: i32) -> Result<(), ()> {
	match how {
		0 /* Read */ => {
			trace!("Shutdown::Read is not implemented");
			Ok(())
		},
		1 /* Write */ => {
			sys_tcp_stream_close(handle)
		},
		2 /* Both */ => {
			sys_tcp_stream_close(handle)
		},
		_ => {
			panic!("Invalid shutdown argument {}", how);
		},
	}
}

#[no_mangle]
pub fn sys_tcp_stream_set_read_timeout(_handle: Handle, _timeout: Option<u64>) -> Result<(), ()> {
	Err(())
}

#[no_mangle]
pub fn sys_tcp_stream_get_read_timeout(_handle: Handle) -> Result<Option<u64>, ()> {
	Err(())
}

#[no_mangle]
pub fn sys_tcp_stream_set_write_timeout(_handle: Handle, _timeout: Option<u64>) -> Result<(), ()> {
	Err(())
}

#[no_mangle]
pub fn sys_tcp_stream_get_write_timeout(_handle: Handle) -> Result<Option<u64>, ()> {
	Err(())
}

#[deprecated(since = "0.1.14", note = "Please don't use this function")]
#[no_mangle]
pub fn sys_tcp_stream_duplicate(_handle: Handle) -> Result<Handle, ()> {
	Err(())
}

#[no_mangle]
pub fn sys_tcp_stream_peek(_handle: Handle, _buf: &mut [u8]) -> Result<usize, ()> {
	Err(())
}

/// If set, this option disables the Nagle algorithm. This means that segments are
/// always sent as soon as possible, even if there is only a small amount of data.
/// When not set, data is buffered until there is a sufficient amount to send out,
/// thereby avoiding the frequent sending of small packets.
#[no_mangle]
pub fn sys_tcp_set_no_delay(handle: Handle, mode: bool) -> Result<(), ()> {
	let mut guard = crate::net::NIC.lock();
	let nic = guard.as_nic_mut().map_err(drop)?;
	let socket = nic.iface.get_socket::<TcpSocket<'_>>(handle);
	socket.set_nagle_enabled(!mode);

	Ok(())
}

#[no_mangle]
pub fn sys_tcp_stream_set_nonblocking(_handle: Handle, mode: bool) -> Result<(), ()> {
	// non-blocking mode is currently not support
	// => return only an error, if `mode` is defined as `true`
	if mode {
		Err(())
	} else {
		Ok(())
	}
}

#[no_mangle]
pub fn sys_tcp_stream_set_tll(_handle: Handle, _ttl: u32) -> Result<(), ()> {
	Err(())
}

#[no_mangle]
pub fn sys_tcp_stream_get_tll(_handle: Handle) -> Result<u32, ()> {
	Err(())
}

#[cfg(feature = "tcp")]
#[no_mangle]
pub fn sys_tcp_stream_peer_addr(handle: Handle) -> Result<(IpAddress, u16), ()> {
	let mut guard = crate::net::NIC.lock();
	let nic = guard.as_nic_mut().map_err(drop)?;
	let socket = nic.iface.get_socket::<TcpSocket<'_>>(handle);
	socket.set_keep_alive(Some(Duration::from_millis(DEFAULT_KEEP_ALIVE_INTERVAL)));
	let endpoint = socket.remote_endpoint();

	Ok((endpoint.addr, endpoint.port))
}

#[cfg(feature = "tcp")]
#[no_mangle]
pub fn sys_tcp_listener_accept(port: u16) -> Result<(Handle, IpAddress, u16), ()> {
	let socket = AsyncSocket::new();
	let (addr, port) = block_on(socket.accept(port), None)?.map_err(|_| ())?;

	Ok((socket.into_inner(), addr, port))
}
