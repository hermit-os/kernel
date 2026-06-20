use alloc::collections::{BTreeMap, btree_map};
use alloc::vec::Vec;
use core::future;
use core::task::Poll;

use hermit_sync::InterruptTicketMutex;
use virtio::vsock::{Hdr, Op, Type};
use virtio::{le16, le32};

#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio as hardware;
#[cfg(feature = "pci")]
use crate::drivers::pci as hardware;
use crate::errno::Errno;
use crate::executor::{WakerRegistration, spawn};
use crate::io;

pub(crate) static VSOCK_MAP: InterruptTicketMutex<VsockMap> =
	InterruptTicketMutex::new(VsockMap::new());

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum VsockState {
	Listen,
	ReceiveRequest,
	Connected,
	Connecting,
	Shutdown,
}

pub(crate) const RAW_SOCKET_BUFFER_SIZE: usize = 256 * 1024;

/// Identifies an established connection by its local (listen) port and the
/// remote endpoint `(remote_cid, remote_port)`. Multiple connections may share
/// one local port, mirroring how TCP demultiplexes by the connection 4-tuple.
pub(crate) type ConnKey = (u32, u32, u32);

#[derive(Debug)]
pub(crate) struct RawSocket {
	pub remote_cid: u32,
	pub remote_port: u32,
	pub fwd_cnt: u32,
	pub peer_fwd_cnt: u32,
	pub peer_buf_alloc: u32,
	pub tx_cnt: u32,
	pub state: VsockState,
	pub rx_waker: WakerRegistration,
	pub tx_waker: WakerRegistration,
	pub buffer: Vec<u8>,
}

impl RawSocket {
	pub fn new(state: VsockState) -> Self {
		Self {
			remote_cid: 0,
			remote_port: 0,
			fwd_cnt: 0,
			peer_fwd_cnt: 0,
			peer_buf_alloc: 0,
			tx_cnt: 0,
			state,
			rx_waker: WakerRegistration::new(),
			tx_waker: WakerRegistration::new(),
			buffer: Vec::with_capacity(RAW_SOCKET_BUFFER_SIZE),
		}
	}
}

async fn vsock_run() {
	future::poll_fn(|cx| {
		let Some(driver) = hardware::get_vsock_driver() else {
			return Poll::Ready(());
		};

		const HEADER_SIZE: usize = size_of::<Hdr>();
		let mut driver_guard = driver.lock();
		let mut hdr: Option<Hdr> = None;
		let mut fwd_cnt: u32 = 0;

		driver_guard.process_packet(|header, data| {
			let op = Op::try_from(header.op.to_ne()).unwrap();
			let port = header.dst_port.to_ne();
			let type_ = Type::try_from(header.type_.to_ne()).unwrap();
			let mut vsock_guard = VSOCK_MAP.lock();
			let header_cid: u32 = header.src_cid.to_ne().try_into().unwrap();
			let remote_port = header.src_port.to_ne();

			// Packets for an established connection address the local listen
			// port but belong to a specific remote endpoint, so route them to
			// the connection entry keyed by `(port, remote_cid, remote_port)`.
			// `Op::Request` (and outbound-connect responses) have no such entry
			// yet and fall back to the listener/connect socket in `port_map`.
			let raw = if let Some(conn) =
				vsock_guard.get_mut_connection((port, header_cid, remote_port))
			{
				conn
			} else if let Some(s) = vsock_guard.get_mut_socket(port) {
				s
			} else {
				return;
			};

			if op == Op::Request && raw.state == VsockState::Listen && type_ == Type::Stream {
				raw.state = VsockState::ReceiveRequest;
				raw.remote_cid = header_cid;
				raw.remote_port = header.src_port.to_ne();
				raw.peer_buf_alloc = header.buf_alloc.to_ne();
				raw.rx_waker.wake();
			} else if (raw.state == VsockState::Connected || raw.state == VsockState::Shutdown)
				&& type_ == Type::Stream
				&& op == Op::Rw
			{
				if raw.remote_cid == header_cid {
					raw.buffer.extend_from_slice(data);
					raw.fwd_cnt = raw.fwd_cnt.wrapping_add(u32::try_from(data.len()).unwrap());
					raw.peer_fwd_cnt = header.fwd_cnt.to_ne();
					raw.tx_waker.wake();
					raw.rx_waker.wake();
					hdr = Some(*header);
					fwd_cnt = raw.fwd_cnt;
				} else {
					trace!("Receive message from invalid source {header_cid}");
				}
			} else if op == Op::CreditUpdate {
				if raw.remote_cid == header_cid {
					raw.peer_fwd_cnt = header.fwd_cnt.to_ne();
					raw.tx_waker.wake();
				} else {
					trace!("Receive message from invalid source {header_cid}");
				}
			} else if op == Op::Shutdown {
				if raw.remote_cid == header_cid {
					raw.state = VsockState::Shutdown;
				} else {
					trace!("Receive message from invalid source {header_cid}");
				}
			} else if op == Op::Response && type_ == Type::Stream {
				if raw.remote_cid == header_cid && raw.state == VsockState::Connecting {
					raw.state = VsockState::Connected;
					raw.peer_buf_alloc = header.buf_alloc.to_ne();
					raw.peer_fwd_cnt = header.fwd_cnt.to_ne();
					raw.tx_waker.wake();
				}
			} else if op == Op::Request {
				// A connection request the listener cannot service right now
				// (e.g. a previous request is still pending accept on this
				// port). Reply with a reset so the peer fails fast instead of
				// blocking until it times out.
				hdr = Some(*header);
				fwd_cnt = raw.fwd_cnt;
			} else if raw.remote_cid == header_cid {
				hdr = Some(*header);
				fwd_cnt = raw.fwd_cnt;
			}
		});

		if let Some(hdr) = hdr {
			driver_guard.send_packet(HEADER_SIZE, |buffer| {
				let response = unsafe { &mut *buffer.as_mut_ptr().cast::<Hdr>() };

				response.src_cid = hdr.dst_cid;
				response.dst_cid = hdr.src_cid;
				response.src_port = hdr.dst_port;
				response.dst_port = hdr.src_port;
				response.len = le32::from_ne(0);
				response.type_ = hdr.type_;
				if hdr.op.to_ne() == u16::from(Op::CreditRequest)
					|| hdr.op.to_ne() == u16::from(Op::Rw)
				{
					response.op = le16::from_ne(Op::CreditUpdate.into());
				} else {
					// reset connection
					response.op = le16::from_ne(Op::Rst.into());
				}
				response.flags = le32::from_ne(0);
				response.buf_alloc = le32::from_ne(RAW_SOCKET_BUFFER_SIZE as u32);
				response.fwd_cnt = le32::from_ne(fwd_cnt);
			});
		}

		// FIXME: only wake when progress can be made
		cx.waker().wake_by_ref();
		Poll::Pending
	})
	.await;
}

pub(crate) struct VsockMap {
	/// Listeners (keyed by listen port) and outbound-connect sockets (keyed by
	/// a synthetic ephemeral port).
	port_map: BTreeMap<u32, RawSocket>,
	/// Established inbound connections, keyed by `(local_port, remote_cid,
	/// remote_port)`, so several connections can share one listen port.
	conn_map: BTreeMap<ConnKey, RawSocket>,
}

impl VsockMap {
	pub const fn new() -> Self {
		Self {
			port_map: BTreeMap::new(),
			conn_map: BTreeMap::new(),
		}
	}

	pub fn bind(&mut self, port: u32) -> io::Result<()> {
		let entry = self.port_map.entry(port);

		match entry {
			btree_map::Entry::Vacant(vacant_entry) => {
				vacant_entry.insert(RawSocket::new(VsockState::Listen));
				Ok(())
			}
			btree_map::Entry::Occupied(_occupied_entry) => Err(Errno::Addrinuse),
		}
	}

	pub fn connect(&mut self, port: u32, cid: u32) -> io::Result<u32> {
		for i in u32::MAX / 4..u32::MAX {
			let mut raw = RawSocket::new(VsockState::Connecting);
			raw.remote_cid = cid;
			raw.remote_port = port;

			if let btree_map::Entry::Vacant(vacant_entry) = self.port_map.entry(i) {
				vacant_entry.insert(raw);
				return Ok(i);
			}
		}

		Err(Errno::Badf)
	}

	pub fn get_mut_socket(&mut self, port: u32) -> Option<&mut RawSocket> {
		self.port_map.get_mut(&port)
	}

	pub fn get_mut_connection(&mut self, key: ConnKey) -> Option<&mut RawSocket> {
		self.conn_map.get_mut(&key)
	}

	pub fn remove_socket(&mut self, port: u32) {
		self.port_map.remove(&port);
	}

	pub fn remove_connection(&mut self, key: ConnKey) {
		self.conn_map.remove(&key);
	}

	/// Move the pending connection on `listen_port` (in state `ReceiveRequest`)
	/// into `conn_map` keyed by `(listen_port, remote_cid, remote_port)`, then
	/// reset the listener entry to `Listen` so it can accept further
	/// connections. Returns the new connection's key.
	pub fn establish(&mut self, listen_port: u32) -> io::Result<ConnKey> {
		let listener = self.port_map.get_mut(&listen_port).ok_or(Errno::Inval)?;
		let key = (listen_port, listener.remote_cid, listener.remote_port);

		// Build the connection entry from the negotiated handshake state, then
		// reset the listener's fields in place. Resetting in place (rather than
		// replacing the whole struct) preserves the listener's wakers, so an
		// `accept()` future already parked on it is not lost.
		let mut conn = RawSocket::new(VsockState::Connected);
		conn.remote_cid = listener.remote_cid;
		conn.remote_port = listener.remote_port;
		conn.peer_buf_alloc = listener.peer_buf_alloc;

		listener.state = VsockState::Listen;
		listener.remote_cid = 0;
		listener.remote_port = 0;
		listener.peer_buf_alloc = 0;

		self.conn_map.insert(key, conn);
		Ok(key)
	}
}

pub(crate) fn init() {
	info!("Try to initialize vsock interface!");

	spawn(vsock_run());
}
