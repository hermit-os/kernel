use alloc::collections::{BTreeMap, VecDeque, btree_map};
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
	/// The peer sent a graceful `Op::Shutdown`. Buffered data may still be
	/// read; once drained, reads report EOF.
	Shutdown,
	/// The peer (or the device) sent an abortive `Op::Rst`. Buffered data may
	/// still be read; once drained, reads report `ECONNRESET`.
	Reset,
}

pub(crate) const RAW_SOCKET_BUFFER_SIZE: usize = 256 * 1024;

/// Default depth of a listener's pending-connection backlog, used when
/// `accept()` is called without an explicit `listen()`. Mirrors the TCP
/// socket's `DEFAULT_BACKLOG`.
pub(crate) const DEFAULT_BACKLOG: usize = 128;

/// Upper bound on a listener's backlog, matching the TCP socket's `SOMAXCONN`
/// (the default maximum used by modern Linux).
pub(crate) const SOMAXCONN: usize = 4096;

/// A pending inbound connection request waiting to be `accept()`ed, captured
/// from an `Op::Request` packet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingRequest {
	pub remote_cid: u32,
	pub remote_port: u32,
	pub peer_buf_alloc: u32,
}

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
	/// Inbound connection requests queued on a listener, awaiting `accept()`.
	/// Only used by listener sockets; empty for connections.
	pub pending: VecDeque<PendingRequest>,
	/// Maximum depth of `pending` for a listener (set by `listen()`, clamped to
	/// `SOMAXCONN`). Further requests beyond this are reset.
	pub backlog: usize,
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
			pending: VecDeque::new(),
			backlog: DEFAULT_BACKLOG,
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

			if op == Op::Request
				&& (raw.state == VsockState::Listen || raw.state == VsockState::ReceiveRequest)
				&& type_ == Type::Stream
			{
				// Queue the inbound request on the listener's backlog so several
				// concurrent connects on the same port can be accepted in turn.
				// `ReceiveRequest` means "at least one request is pending accept".
				if raw.pending.len() < raw.backlog {
					raw.pending.push_back(PendingRequest {
						remote_cid: header_cid,
						remote_port: header.src_port.to_ne(),
						peer_buf_alloc: header.buf_alloc.to_ne(),
					});
					raw.state = VsockState::ReceiveRequest;
					raw.rx_waker.wake();
				} else {
					// Backlog full: reset so the peer backs off and retries.
					hdr = Some(*header);
					fwd_cnt = raw.fwd_cnt;
				}
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
					raw.rx_waker.wake();
					raw.tx_waker.wake();
				} else {
					trace!("Receive message from invalid source {header_cid}");
				}
			} else if op == Op::Rst {
				if raw.remote_cid == header_cid {
					raw.state = VsockState::Reset;
					raw.rx_waker.wake();
					raw.tx_waker.wake();
				} else {
					trace!("Receive message from invalid source {header_cid}");
				}
			} else if op == Op::Response && type_ == Type::Stream {
				if raw.remote_cid == header_cid && raw.state == VsockState::Connecting {
					raw.state = VsockState::Connected;
					raw.peer_buf_alloc = header.buf_alloc.to_ne();
					raw.peer_fwd_cnt = header.fwd_cnt.to_ne();
					// The blocking `connect()` future parks on `rx_waker` (see
					// `Socket::connect`'s Connecting arm), so it MUST be woken
					// here or the connect never returns. Wake `tx_waker` too:
					// a freshly-connected socket is immediately writable.
					raw.rx_waker.wake();
					raw.tx_waker.wake();
				}
			} else if op == Op::Request {
				// A request that did not match the listener backlog arm above
				// (e.g. addressed to a non-listening socket). Reset so the peer
				// fails fast instead of blocking until it times out.
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

	/// Set the pending-connection backlog depth for the listener on `port`,
	/// clamped to `SOMAXCONN`.
	pub fn set_backlog(&mut self, port: u32, backlog: usize) -> io::Result<()> {
		let listener = self.port_map.get_mut(&port).ok_or(Errno::Inval)?;
		listener.backlog = backlog.min(SOMAXCONN);
		Ok(())
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

	/// Pop the next pending request from the listener on `listen_port`'s backlog,
	/// move it into `conn_map` keyed by `(listen_port, remote_cid, remote_port)`,
	/// and return the new connection's key. The listener stays in
	/// `ReceiveRequest` while more requests remain queued, otherwise returns to
	/// `Listen`. Resetting fields in place preserves the listener's wakers, so an
	/// `accept()` future already parked on it is not lost.
	pub fn establish(&mut self, listen_port: u32) -> io::Result<ConnKey> {
		let listener = self.port_map.get_mut(&listen_port).ok_or(Errno::Inval)?;
		let req = listener.pending.pop_front().ok_or(Errno::Again)?;
		let key = (listen_port, req.remote_cid, req.remote_port);

		let mut conn = RawSocket::new(VsockState::Connected);
		conn.remote_cid = req.remote_cid;
		conn.remote_port = req.remote_port;
		conn.peer_buf_alloc = req.peer_buf_alloc;

		// Stay in ReceiveRequest if more requests are queued so `accept()` keeps
		// draining them; otherwise go back to plain Listen.
		listener.state = if listener.pending.is_empty() {
			VsockState::Listen
		} else {
			VsockState::ReceiveRequest
		};

		self.conn_map.insert(key, conn);
		Ok(key)
	}
}

pub(crate) fn init() {
	info!("Try to initialize vsock interface!");

	spawn(vsock_run());
}
