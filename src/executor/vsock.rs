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
		if let Some(driver) = hardware::get_vsock_driver() {
			const HEADER_SIZE: usize = core::mem::size_of::<Hdr>();
			let mut driver_guard = driver.lock();
			let mut hdr: Option<Hdr> = None;
			let mut fwd_cnt: u32 = 0;

			driver_guard.process_packet(|header, data| {
				let op = Op::try_from(header.op.to_ne()).unwrap();
				let port = header.dst_port.to_ne();
				let type_ = Type::try_from(header.type_.to_ne()).unwrap();
				let mut vsock_guard = VSOCK_MAP.lock();
				let header_cid: u32 = header.src_cid.to_ne().try_into().unwrap();

				if let Some(raw) = vsock_guard.get_mut_socket(port) {
					if op == Op::Request && raw.state == VsockState::Listen && type_ == Type::Stream
					{
						raw.state = VsockState::ReceiveRequest;
						raw.remote_cid = header_cid;
						raw.remote_port = header.src_port.to_ne();
						raw.peer_buf_alloc = header.buf_alloc.to_ne();
						raw.rx_waker.wake();
					} else if (raw.state == VsockState::Connected
						|| raw.state == VsockState::Shutdown)
						&& type_ == Type::Stream
						&& op == Op::Rw
					{
						if raw.remote_cid == header_cid {
							raw.buffer.extend_from_slice(data);
							raw.fwd_cnt =
								raw.fwd_cnt.wrapping_add(u32::try_from(data.len()).unwrap());
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
						}
					} else if raw.remote_cid == header_cid {
						hdr = Some(*header);
						fwd_cnt = raw.fwd_cnt;
					}
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
		} else {
			Poll::Ready(())
		}
	})
	.await;
}

pub(crate) struct VsockMap {
	port_map: BTreeMap<u32, RawSocket>,
}

impl VsockMap {
	pub const fn new() -> Self {
		Self {
			port_map: BTreeMap::new(),
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

	pub fn get_socket(&self, port: u32) -> Option<&RawSocket> {
		self.port_map.get(&port)
	}

	pub fn get_mut_socket(&mut self, port: u32) -> Option<&mut RawSocket> {
		self.port_map.get_mut(&port)
	}

	pub fn remove_socket(&mut self, port: u32) {
		let _ = self.port_map.remove(&port);
	}
}

pub(crate) fn init() {
	info!("Try to initialize vsock interface!");

	spawn(vsock_run());
}
