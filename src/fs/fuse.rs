use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::Poll;
use core::{future, mem};

use async_lock::Mutex;
use async_trait::async_trait;
use fuse_abi::linux::*;
use zerocopy::FromBytes;

use crate::alloc::string::ToString;
#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio::get_filesystem_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_filesystem_driver;
use crate::drivers::virtio::virtqueue::error::VirtqError;
use crate::executor::block_on;
use crate::fd::PollEvent;
use crate::fs::{
	self, AccessPermission, DirectoryEntry, FileAttr, NodeKind, ObjectInterface, OpenOption,
	SeekWhence, VfsNode,
};
use crate::mm::device_alloc::DeviceAlloc;
use crate::time::{time_t, timespec};
use crate::{arch, io};

// response out layout eg @ https://github.com/zargony/fuse-rs/blob/bf6d1cf03f3277e35b580f3c7b9999255d72ecf3/src/ll/request.rs#L44
// op in/out sizes/layout: https://github.com/hanwen/go-fuse/blob/204b45dba899dfa147235c255908236d5fde2d32/fuse/opcode.go#L439
// possible responses for command: qemu/tools/virtiofsd/fuse_lowlevel.h

const MAX_READ_LEN: usize = 1024 * 64;
const MAX_WRITE_LEN: usize = 1024 * 64;

const U64_SIZE: usize = mem::size_of::<u64>();

const S_IFLNK: u32 = 0o120_000;
const S_IFMT: u32 = 0o170_000;

pub(crate) trait FuseInterface {
	fn send_command<O: ops::Op + 'static>(
		&mut self,
		cmd: Cmd<O>,
		rsp_payload_len: u32,
	) -> Result<Rsp<O>, VirtqError>
	where
		<O as ops::Op>::InStruct: Send,
		<O as ops::Op>::OutStruct: Send;

	fn get_mount_point(&self) -> String;
}

pub(crate) mod ops {
	#![allow(clippy::type_complexity)]
	use alloc::boxed::Box;
	use alloc::ffi::CString;

	use fuse_abi::linux::*;

	use super::Cmd;
	use crate::fd::PollEvent;
	use crate::fs::SeekWhence;

	#[repr(C)]
	#[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
	pub(crate) struct CreateOut {
		pub entry: fuse_entry_out,
		pub open: fuse_open_out,
	}

	pub(crate) trait Op {
		const OP_CODE: fuse_opcode;

		type InStruct: core::fmt::Debug;
		type InPayload: ?Sized;
		type OutStruct: core::fmt::Debug;
		type OutPayload: ?Sized;
	}

	#[derive(Debug)]
	pub(crate) struct Init;

	impl Op for Init {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_INIT;
		type InStruct = fuse_init_in;
		type InPayload = ();
		type OutStruct = fuse_init_out;
		type OutPayload = ();
	}

	impl Init {
		pub(crate) fn create() -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(
				FUSE_ROOT_ID,
				fuse_init_in {
					major: 7,
					minor: 31,
					..Default::default()
				},
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Create;

	impl Op for Create {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_CREATE;
		type InStruct = fuse_create_in;
		type InPayload = CString;
		type OutStruct = CreateOut;
		type OutPayload = ();
	}

	impl Create {
		#[allow(clippy::self_named_constructors)]
		pub(crate) fn create(path: CString, flags: u32, mode: u32) -> (Cmd<Self>, u32) {
			let cmd = Cmd::with_cstring(
				FUSE_ROOT_ID,
				fuse_create_in {
					flags,
					mode,
					..Default::default()
				},
				path,
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Open;

	impl Op for Open {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_OPEN;
		type InStruct = fuse_open_in;
		type InPayload = ();
		type OutStruct = fuse_open_out;
		type OutPayload = ();
	}

	impl Open {
		pub(crate) fn create(nid: u64, flags: u32) -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(
				nid,
				fuse_open_in {
					flags,
					..Default::default()
				},
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Write;

	impl Op for Write {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_WRITE;
		type InStruct = fuse_write_in;
		type InPayload = [u8];
		type OutStruct = fuse_write_out;
		type OutPayload = ();
	}

	impl Write {
		pub(crate) fn create(nid: u64, fh: u64, buf: Box<[u8]>, offset: u64) -> (Cmd<Self>, u32) {
			let cmd = Cmd::with_boxed_slice(
				nid,
				fuse_write_in {
					fh,
					offset,
					size: buf.len().try_into().unwrap(),
					..Default::default()
				},
				buf,
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Read;

	impl Op for Read {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_READ;
		type InStruct = fuse_read_in;
		type InPayload = ();
		type OutStruct = ();
		type OutPayload = [u8];
	}

	impl Read {
		pub(crate) fn create(nid: u64, fh: u64, size: u32, offset: u64) -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(
				nid,
				fuse_read_in {
					fh,
					offset,
					size,
					..Default::default()
				},
			);
			(cmd, size)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Lseek;

	impl Op for Lseek {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_LSEEK;
		type InStruct = fuse_lseek_in;
		type InPayload = ();
		type OutStruct = fuse_lseek_out;
		type OutPayload = ();
	}

	impl Lseek {
		pub(crate) fn create(
			nid: u64,
			fh: u64,
			offset: isize,
			whence: SeekWhence,
		) -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(
				nid,
				fuse_lseek_in {
					fh,
					offset: i64::try_from(offset).unwrap() as u64,
					whence: u8::from(whence).into(),
					..Default::default()
				},
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Getattr;

	impl Op for Getattr {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_GETATTR;
		type InStruct = fuse_getattr_in;
		type InPayload = ();
		type OutStruct = fuse_attr_out;
		type OutPayload = ();
	}

	impl Getattr {
		pub(crate) fn create(nid: u64, fh: u64, getattr_flags: u32) -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(
				nid,
				fuse_getattr_in {
					getattr_flags,
					fh,
					..Default::default()
				},
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Readlink;

	impl Op for Readlink {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_READLINK;
		type InStruct = ();
		type InPayload = ();
		type OutStruct = ();
		type OutPayload = [u8];
	}

	impl Readlink {
		pub(crate) fn create(nid: u64, size: u32) -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(nid, ());
			(cmd, size)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Release;

	impl Op for Release {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_RELEASE;
		type InStruct = fuse_release_in;
		type InPayload = ();
		type OutStruct = ();
		type OutPayload = ();
	}

	impl Release {
		pub(crate) fn create(nid: u64, fh: u64) -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(
				nid,
				fuse_release_in {
					fh,
					..Default::default()
				},
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Poll;

	impl Op for Poll {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_POLL;
		type InStruct = fuse_poll_in;
		type InPayload = ();
		type OutStruct = fuse_poll_out;
		type OutPayload = ();
	}

	impl Poll {
		pub(crate) fn create(nid: u64, fh: u64, kh: u64, event: PollEvent) -> (Cmd<Self>, u32) {
			let cmd = Cmd::new(
				nid,
				fuse_poll_in {
					fh,
					kh,
					events: event.bits() as u32,
					..Default::default()
				},
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Mkdir;

	impl Op for Mkdir {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_MKDIR;
		type InStruct = fuse_mkdir_in;
		type InPayload = CString;
		type OutStruct = fuse_entry_out;
		type OutPayload = ();
	}

	impl Mkdir {
		pub(crate) fn create(path: CString, mode: u32) -> (Cmd<Self>, u32) {
			let cmd = Cmd::with_cstring(
				FUSE_ROOT_ID,
				fuse_mkdir_in {
					mode,
					..Default::default()
				},
				path,
			);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Unlink;

	impl Op for Unlink {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_UNLINK;
		type InStruct = ();
		type InPayload = CString;
		type OutStruct = ();
		type OutPayload = ();
	}

	impl Unlink {
		pub(crate) fn create(name: CString) -> (Cmd<Self>, u32) {
			let cmd = Cmd::with_cstring(FUSE_ROOT_ID, (), name);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Rmdir;

	impl Op for Rmdir {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_RMDIR;
		type InStruct = ();
		type InPayload = CString;
		type OutStruct = ();
		type OutPayload = ();
	}

	impl Rmdir {
		pub(crate) fn create(name: CString) -> (Cmd<Self>, u32) {
			let cmd = Cmd::with_cstring(FUSE_ROOT_ID, (), name);
			(cmd, 0)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Lookup;

	impl Op for Lookup {
		const OP_CODE: fuse_opcode = fuse_opcode::FUSE_LOOKUP;
		type InStruct = ();
		type InPayload = CString;
		type OutStruct = ();
		// Lookups return [fuse_entry_out] only when there actually is a result. For this reason,
		// it is not part of the header (since all other headers are always there).
		type OutPayload = [u8];
	}

	impl Lookup {
		pub(crate) fn create(name: CString) -> (Cmd<Self>, u32) {
			let cmd = Cmd::with_cstring(FUSE_ROOT_ID, (), name);
			(cmd, size_of::<fuse_entry_out>().try_into().unwrap())
		}
	}
}

impl From<fuse_attr> for FileAttr {
	fn from(attr: fuse_attr) -> FileAttr {
		FileAttr {
			st_ino: attr.ino,
			st_nlink: attr.nlink.into(),
			st_mode: AccessPermission::from_bits_retain(attr.mode),
			st_uid: attr.uid,
			st_gid: attr.gid,
			st_rdev: attr.rdev.into(),
			st_size: attr.size.try_into().unwrap(),
			st_blksize: attr.blksize.into(),
			st_blocks: attr.blocks.try_into().unwrap(),
			st_atim: timespec {
				tv_sec: attr.atime as time_t,
				tv_nsec: attr.atimensec as i32,
			},
			st_mtim: timespec {
				tv_sec: attr.mtime as time_t,
				tv_nsec: attr.mtimensec as i32,
			},
			st_ctim: timespec {
				tv_sec: attr.ctime as time_t,
				tv_nsec: attr.ctimensec as i32,
			},
			..Default::default()
		}
	}
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct CmdHeader<O: ops::Op> {
	pub in_header: fuse_in_header,
	op_header: O::InStruct,
}

impl<O: ops::Op> CmdHeader<O>
where
	O: ops::Op<InPayload = ()>,
{
	fn new(nodeid: u64, op_header: O::InStruct) -> Self {
		Self::with_payload_size(nodeid, op_header, 0)
	}
}

impl<O: ops::Op> CmdHeader<O> {
	fn with_payload_size(nodeid: u64, op_header: O::InStruct, len: usize) -> CmdHeader<O> {
		CmdHeader {
			in_header: fuse_in_header {
				// The length we need the provide in the header is not the same as the size of the struct because of padding, so we need to calculate it manually.
				len: (core::mem::size_of::<fuse_in_header>()
					+ core::mem::size_of::<O::InStruct>()
					+ len)
					.try_into()
					.expect("The command is too large"),
				opcode: O::OP_CODE.into(),
				nodeid,
				unique: 1,
				..Default::default()
			},
			op_header,
		}
	}
}

pub(crate) struct Cmd<O: ops::Op> {
	pub headers: Box<CmdHeader<O>, DeviceAlloc>,
	pub payload: Option<Vec<u8, DeviceAlloc>>,
}

impl<O: ops::Op> Cmd<O>
where
	O: ops::Op<InPayload = ()>,
{
	fn new(nodeid: u64, op_header: O::InStruct) -> Self {
		Self {
			headers: Box::new_in(CmdHeader::new(nodeid, op_header), DeviceAlloc),
			payload: None,
		}
	}
}

impl<O: ops::Op> Cmd<O>
where
	O: ops::Op<InPayload = CString>,
{
	fn with_cstring(nodeid: u64, op_header: O::InStruct, cstring: CString) -> Self {
		let cstring_bytes = cstring.into_bytes_with_nul().to_vec_in(DeviceAlloc);
		Self {
			headers: Box::new_in(
				CmdHeader::with_payload_size(nodeid, op_header, cstring_bytes.len()),
				DeviceAlloc,
			),
			payload: Some(cstring_bytes),
		}
	}
}

impl<O: ops::Op> Cmd<O>
where
	O: ops::Op<InPayload = [u8]>,
{
	fn with_boxed_slice(nodeid: u64, op_header: O::InStruct, slice: Box<[u8]>) -> Self {
		let mut device_slice = Vec::with_capacity_in(slice.len(), DeviceAlloc);
		device_slice.extend_from_slice(&slice);
		Self {
			headers: Box::new_in(
				CmdHeader::with_payload_size(nodeid, op_header, slice.len()),
				DeviceAlloc,
			),
			payload: Some(device_slice),
		}
	}
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct RspHeader<O: ops::Op> {
	out_header: fuse_out_header,
	op_header: O::OutStruct,
}

#[derive(Debug)]
pub(crate) struct Rsp<O: ops::Op> {
	pub headers: Box<RspHeader<O>, DeviceAlloc>,
	pub payload: Option<Vec<u8, DeviceAlloc>>,
}

fn lookup(name: CString) -> Option<u64> {
	let (cmd, rsp_payload_len) = ops::Lookup::create(name);
	let rsp = get_filesystem_driver()
		.unwrap()
		.lock()
		.send_command(cmd, rsp_payload_len)
		.ok()?;
	if rsp.headers.out_header.error == 0 {
		let entry_out = fuse_entry_out::ref_from_bytes(rsp.payload.as_ref().unwrap()).unwrap();
		Some(entry_out.nodeid)
	} else {
		None
	}
}

fn readlink(nid: u64) -> io::Result<String> {
	let len = MAX_READ_LEN as u32;
	let (cmd, rsp_payload_len) = ops::Readlink::create(nid, len);
	let rsp = get_filesystem_driver()
		.unwrap()
		.lock()
		.send_command(cmd, rsp_payload_len)?;
	let len: usize = if rsp.headers.out_header.len as usize - mem::size_of::<fuse_out_header>()
		>= usize::try_from(len).unwrap()
	{
		len.try_into().unwrap()
	} else {
		(rsp.headers.out_header.len as usize) - mem::size_of::<fuse_out_header>()
	};

	Ok(String::from_utf8(rsp.payload.unwrap()[..len].to_vec()).unwrap())
}

#[derive(Debug)]
struct FuseFileHandleInner {
	fuse_nid: Option<u64>,
	fuse_fh: Option<u64>,
	offset: usize,
}

impl FuseFileHandleInner {
	pub fn new() -> Self {
		Self {
			fuse_nid: None,
			fuse_fh: None,
			offset: 0,
		}
	}

	async fn poll(&self, events: PollEvent) -> io::Result<PollEvent> {
		static KH: AtomicU64 = AtomicU64::new(0);
		let kh = KH.fetch_add(1, Ordering::SeqCst);

		future::poll_fn(|cx| {
			if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
				let (cmd, rsp_payload_len) = ops::Poll::create(nid, fh, kh, events);
				let rsp = get_filesystem_driver()
					.ok_or(io::Error::ENOSYS)?
					.lock()
					.send_command(cmd, rsp_payload_len)?;

				if rsp.headers.out_header.error < 0 {
					Poll::Ready(Err(io::Error::EIO))
				} else {
					let revents =
						PollEvent::from_bits(i16::try_from(rsp.headers.op_header.revents).unwrap())
							.unwrap();
					if !revents.intersects(events)
						&& !revents.intersects(
							PollEvent::POLLERR | PollEvent::POLLNVAL | PollEvent::POLLHUP,
						) {
						// the current implementation use polling to wait for an event
						// consequently, we have to wakeup the waker, if the the event doesn't arrive
						cx.waker().wake_by_ref();
					}
					Poll::Ready(Ok(revents))
				}
			} else {
				Poll::Ready(Ok(PollEvent::POLLERR))
			}
		})
		.await
	}

	fn read(&mut self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		let mut len = buf.len();
		if len > MAX_READ_LEN {
			debug!("Reading longer than max_read_len: {len}");
			len = MAX_READ_LEN;
		}
		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, rsp_payload_len) =
				ops::Read::create(nid, fh, len.try_into().unwrap(), self.offset as u64);
			let rsp = get_filesystem_driver()
				.ok_or(io::Error::ENOSYS)?
				.lock()
				.send_command(cmd, rsp_payload_len)?;
			let len: usize =
				if (rsp.headers.out_header.len as usize) - mem::size_of::<fuse_out_header>() >= len
				{
					len
				} else {
					(rsp.headers.out_header.len as usize) - mem::size_of::<fuse_out_header>()
				};
			self.offset += len;

			buf[..len].write_copy_of_slice(&rsp.payload.unwrap()[..len]);

			Ok(len)
		} else {
			debug!("File not open, cannot read!");
			Err(io::Error::ENOENT)
		}
	}

	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		debug!("FUSE write!");
		let mut truncated_len = buf.len();
		if truncated_len > MAX_WRITE_LEN {
			debug!(
				"Writing longer than max_write_len: {} > {}",
				buf.len(),
				MAX_WRITE_LEN
			);
			truncated_len = MAX_WRITE_LEN;
		}
		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let truncated_buf = Box::<[u8]>::from(&buf[..truncated_len]);
			let (cmd, rsp_payload_len) =
				ops::Write::create(nid, fh, truncated_buf, self.offset as u64);
			let rsp = get_filesystem_driver()
				.ok_or(io::Error::ENOSYS)?
				.lock()
				.send_command(cmd, rsp_payload_len)?;

			if rsp.headers.out_header.error < 0 {
				return Err(io::Error::EIO);
			}

			let rsp_size = rsp.headers.op_header.size;
			let rsp_len: usize = if rsp_size > u32::try_from(truncated_len).unwrap() {
				truncated_len
			} else {
				rsp_size.try_into().unwrap()
			};
			self.offset += rsp_len;
			Ok(rsp_len)
		} else {
			warn!("File not open, cannot read!");
			Err(io::Error::ENOENT)
		}
	}

	fn lseek(&mut self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		debug!("FUSE lseek");

		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, rsp_payload_len) = ops::Lseek::create(nid, fh, offset, whence);
			let rsp = get_filesystem_driver()
				.ok_or(io::Error::ENOSYS)?
				.lock()
				.send_command(cmd, rsp_payload_len)?;

			if rsp.headers.out_header.error < 0 {
				return Err(io::Error::EIO);
			}

			let rsp_offset = rsp.headers.op_header.offset;
			self.offset = rsp.headers.op_header.offset.try_into().unwrap();

			Ok(rsp_offset.try_into().unwrap())
		} else {
			Err(io::Error::EIO)
		}
	}

	fn fstat(&mut self) -> io::Result<FileAttr> {
		debug!("FUSE getattr");
		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, rsp_payload_len) = ops::Getattr::create(nid, fh, FUSE_GETATTR_FH);
			let rsp = get_filesystem_driver()
				.ok_or(io::Error::ENOSYS)?
				.lock()
				.send_command(cmd, rsp_payload_len)?;
			if rsp.headers.out_header.error < 0 {
				return Err(io::Error::EIO);
			}
			Ok(rsp.headers.op_header.attr.into())
		} else {
			Err(io::Error::EIO)
		}
	}
}

impl Drop for FuseFileHandleInner {
	fn drop(&mut self) {
		if self.fuse_nid.is_some() && self.fuse_fh.is_some() {
			let (cmd, rsp_payload_len) =
				ops::Release::create(self.fuse_nid.unwrap(), self.fuse_fh.unwrap());
			get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp_payload_len)
				.unwrap();
		}
	}
}

#[derive(Debug)]
struct FuseFileHandle(pub Arc<Mutex<FuseFileHandleInner>>);

impl FuseFileHandle {
	pub fn new() -> Self {
		Self(Arc::new(Mutex::new(FuseFileHandleInner::new())))
	}
}

#[async_trait]
impl ObjectInterface for FuseFileHandle {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		self.0.lock().await.poll(event).await
	}

	async fn read(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		self.0.lock().await.read(buf)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		self.0.lock().await.write(buf)
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		self.0.lock().await.lseek(offset, whence)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		self.0.lock().await.fstat()
	}
}

impl Clone for FuseFileHandle {
	fn clone(&self) -> Self {
		warn!("FuseFileHandle: clone not tested");
		Self(self.0.clone())
	}
}

#[derive(Debug, Clone)]
pub struct FuseDirectoryHandle {
	name: Option<String>,
}

impl FuseDirectoryHandle {
	pub fn new(name: Option<String>) -> Self {
		Self { name }
	}
}

#[async_trait]
impl ObjectInterface for FuseDirectoryHandle {
	async fn readdir(&self) -> io::Result<Vec<DirectoryEntry>> {
		let path: CString = if let Some(name) = &self.name {
			CString::new("/".to_string() + name).unwrap()
		} else {
			CString::new("/".to_string()).unwrap()
		};

		debug!("FUSE opendir: {path:#?}");

		let fuse_nid = lookup(path.clone()).ok_or(io::Error::ENOENT)?;

		// Opendir
		// Flag 0x10000 for O_DIRECTORY might not be necessary
		let (mut cmd, rsp_payload_len) = ops::Open::create(fuse_nid, 0x10000);
		cmd.headers.in_header.opcode = fuse_opcode::FUSE_OPENDIR as u32;
		let rsp = get_filesystem_driver()
			.ok_or(io::Error::ENOSYS)?
			.lock()
			.send_command(cmd, rsp_payload_len)?;
		let fuse_fh = rsp.headers.op_header.fh;

		debug!("FUSE readdir: {path:#?}");

		// Linux seems to allocate a single page to store the dirfile
		let len = MAX_READ_LEN as u32;
		let mut offset: usize = 0;

		// read content of the directory
		let (mut cmd, rsp_payload_len) = ops::Read::create(fuse_nid, fuse_fh, len, 0);
		cmd.headers.in_header.opcode = fuse_opcode::FUSE_READDIR as u32;
		let rsp = get_filesystem_driver()
			.ok_or(io::Error::ENOSYS)?
			.lock()
			.send_command(cmd, rsp_payload_len)?;

		let len: usize = if rsp.headers.out_header.len as usize - mem::size_of::<fuse_out_header>()
			>= usize::try_from(len).unwrap()
		{
			len.try_into().unwrap()
		} else {
			(rsp.headers.out_header.len as usize) - mem::size_of::<fuse_out_header>()
		};

		if len <= core::mem::size_of::<fuse_dirent>() {
			debug!("FUSE no new dirs");
			return Err(io::Error::ENOENT);
		}

		let mut entries: Vec<DirectoryEntry> = Vec::new();
		while (rsp.headers.out_header.len as usize) - offset > core::mem::size_of::<fuse_dirent>() {
			let dirent = unsafe {
				&*rsp
					.payload
					.as_ref()
					.unwrap()
					.as_ptr()
					.byte_add(offset)
					.cast::<fuse_dirent>()
			};

			offset += core::mem::size_of::<fuse_dirent>() + dirent.namelen as usize;
			// Align to dirent struct
			offset = ((offset) + U64_SIZE - 1) & (!(U64_SIZE - 1));

			let name: &'static [u8] = unsafe {
				core::slice::from_raw_parts(
					dirent.name.as_ptr().cast(),
					dirent.namelen.try_into().unwrap(),
				)
			};
			entries.push(DirectoryEntry::new(unsafe {
				core::str::from_utf8_unchecked(name).to_string()
			}));
		}

		let (cmd, rsp_payload_len) = ops::Release::create(fuse_nid, fuse_fh);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp_payload_len)?;

		Ok(entries)
	}
}

#[derive(Debug)]
pub(crate) struct FuseDirectory {
	prefix: Option<String>,
	attr: FileAttr,
}

impl FuseDirectory {
	pub fn new(prefix: Option<String>) -> Self {
		let microseconds = arch::kernel::systemtime::now_micros();
		let t = timespec::from_usec(microseconds as i64);

		FuseDirectory {
			prefix,
			attr: FileAttr {
				st_mode: AccessPermission::from_bits(0o777).unwrap() | AccessPermission::S_IFDIR,
				st_atim: t,
				st_mtim: t,
				st_ctim: t,
				..Default::default()
			},
		}
	}

	fn traversal_path(&self, components: &[&str]) -> CString {
		let prefix_deref = self.prefix.as_deref();
		let components_with_prefix = prefix_deref.iter().chain(components.iter().rev());
		let path: String = components_with_prefix
			.flat_map(|component| ["/", component])
			.collect();
		if path.is_empty() {
			CString::new("/").unwrap()
		} else {
			CString::new(path).unwrap()
		}
	}
}

impl VfsNode for FuseDirectory {
	/// Returns the node type
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		Ok(self.attr)
	}

	fn get_object(&self) -> io::Result<Arc<dyn ObjectInterface>> {
		Ok(Arc::new(FuseDirectoryHandle::new(self.prefix.clone())))
	}

	fn traverse_readdir(&self, components: &mut Vec<&str>) -> io::Result<Vec<DirectoryEntry>> {
		let path = self.traversal_path(components);

		debug!("FUSE opendir: {path:#?}");

		let fuse_nid = lookup(path.clone()).ok_or(io::Error::ENOENT)?;

		// Opendir
		// Flag 0x10000 for O_DIRECTORY might not be necessary
		let (mut cmd, rsp_payload_len) = ops::Open::create(fuse_nid, 0x10000);
		cmd.headers.in_header.opcode = fuse_opcode::FUSE_OPENDIR as u32;
		let rsp = get_filesystem_driver()
			.ok_or(io::Error::ENOSYS)?
			.lock()
			.send_command(cmd, rsp_payload_len)?;
		let fuse_fh = rsp.headers.op_header.fh;

		debug!("FUSE readdir: {path:#?}");

		// Linux seems to allocate a single page to store the dirfile
		let len = MAX_READ_LEN as u32;
		let mut offset: usize = 0;

		// read content of the directory
		let (mut cmd, rsp_payload_len) = ops::Read::create(fuse_nid, fuse_fh, len, 0);
		cmd.headers.in_header.opcode = fuse_opcode::FUSE_READDIR as u32;
		let rsp = get_filesystem_driver()
			.ok_or(io::Error::ENOSYS)?
			.lock()
			.send_command(cmd, rsp_payload_len)?;

		let len: usize = if rsp.headers.out_header.len as usize - mem::size_of::<fuse_out_header>()
			>= usize::try_from(len).unwrap()
		{
			len.try_into().unwrap()
		} else {
			(rsp.headers.out_header.len as usize) - mem::size_of::<fuse_out_header>()
		};

		if len <= core::mem::size_of::<fuse_dirent>() {
			debug!("FUSE no new dirs");
			return Err(io::Error::ENOENT);
		}

		let mut entries: Vec<DirectoryEntry> = Vec::new();
		while (rsp.headers.out_header.len as usize) - offset > core::mem::size_of::<fuse_dirent>() {
			let dirent = unsafe {
				&*rsp
					.payload
					.as_ref()
					.unwrap()
					.as_ptr()
					.byte_add(offset)
					.cast::<fuse_dirent>()
			};

			offset += core::mem::size_of::<fuse_dirent>() + dirent.namelen as usize;
			// Align to dirent struct
			offset = ((offset) + U64_SIZE - 1) & (!(U64_SIZE - 1));

			let name: &'static [u8] = unsafe {
				core::slice::from_raw_parts(
					dirent.name.as_ptr().cast(),
					dirent.namelen.try_into().unwrap(),
				)
			};
			entries.push(DirectoryEntry::new(unsafe {
				core::str::from_utf8_unchecked(name).to_string()
			}));
		}

		let (cmd, rsp_payload_len) = ops::Release::create(fuse_nid, fuse_fh);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp_payload_len)?;

		Ok(entries)
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		let path = self.traversal_path(components);

		debug!("FUSE stat: {path:#?}");

		// Is there a better way to implement this?
		let (cmd, rsp_payload_len) = ops::Lookup::create(path);
		let rsp = get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp_payload_len)?;

		if rsp.headers.out_header.error != 0 {
			return Err(io::Error::try_from(-rsp.headers.out_header.error).unwrap());
		}

		let entry_out = fuse_entry_out::ref_from_bytes(rsp.payload.as_ref().unwrap()).unwrap();
		let attr = entry_out.attr;

		if attr.mode & S_IFMT != S_IFLNK {
			return Ok(FileAttr::from(attr));
		}

		let path = readlink(entry_out.nodeid)?;
		let mut components: Vec<&str> = path.split('/').collect();
		self.traverse_stat(&mut components)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		let path = self.traversal_path(components);

		debug!("FUSE lstat: {path:#?}");

		let (cmd, rsp_payload_len) = ops::Lookup::create(path);
		let rsp = get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp_payload_len)?;

		if rsp.headers.out_header.error != 0 {
			Err(io::Error::try_from(-rsp.headers.out_header.error).unwrap())
		} else {
			let entry_out = fuse_entry_out::ref_from_bytes(rsp.payload.as_ref().unwrap()).unwrap();
			Ok(FileAttr::from(entry_out.attr))
		}
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<dyn ObjectInterface>> {
		let path = self.traversal_path(components);

		debug!("FUSE open: {path:#?}, {opt:?} {mode:?}");

		if opt.contains(OpenOption::O_DIRECTORY) {
			if opt.contains(OpenOption::O_CREAT) {
				// See https://lwn.net/Articles/926782/
				warn!("O_DIRECTORY and O_CREAT are together invalid as open options.");
				return Err(io::Error::EINVAL);
			}

			let (cmd, rsp_payload_len) = ops::Lookup::create(path.clone());
			let rsp = get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp_payload_len)?;

			if rsp.headers.out_header.error == 0 {
				let entry_out =
					fuse_entry_out::ref_from_bytes(rsp.payload.as_ref().unwrap()).unwrap();
				let attr = FileAttr::from(entry_out.attr);
				if attr.st_mode.contains(AccessPermission::S_IFDIR) {
					let mut path = path.into_string().unwrap();
					path.remove(0);
					Ok(Arc::new(FuseDirectoryHandle::new(Some(path))))
				} else {
					Err(io::Error::ENOTDIR)
				}
			} else {
				Err(io::Error::try_from(-rsp.headers.out_header.error).unwrap())
			}
		} else {
			let file = FuseFileHandle::new();

			// 1.FUSE_INIT to create session
			// Already done
			let mut file_guard = block_on(async { Ok(file.0.lock().await) }, None)?;

			// Differentiate between opening and creating new file, since fuse does not support O_CREAT on open.
			if opt.contains(OpenOption::O_CREAT) {
				// Create file (opens implicitly, returns results from both lookup and open calls)
				let (cmd, rsp_payload_len) =
					ops::Create::create(path, opt.bits().try_into().unwrap(), mode.bits());
				let rsp = get_filesystem_driver()
					.ok_or(io::Error::ENOSYS)?
					.lock()
					.send_command(cmd, rsp_payload_len)?;

				let inner = rsp.headers.op_header;
				file_guard.fuse_nid = Some(inner.entry.nodeid);
				file_guard.fuse_fh = Some(inner.open.fh);
			} else {
				// 2.FUSE_LOOKUP(FUSE_ROOT_ID, “foo”) -> nodeid
				file_guard.fuse_nid = lookup(path);

				if file_guard.fuse_nid.is_none() {
					warn!("Fuse lookup seems to have failed!");
					return Err(io::Error::ENOENT);
				}

				// 3.FUSE_OPEN(nodeid, O_RDONLY) -> fh
				let (cmd, rsp_payload_len) =
					ops::Open::create(file_guard.fuse_nid.unwrap(), opt.bits().try_into().unwrap());
				let rsp = get_filesystem_driver()
					.ok_or(io::Error::ENOSYS)?
					.lock()
					.send_command(cmd, rsp_payload_len)?;
				file_guard.fuse_fh = Some(rsp.headers.op_header.fh);
			}

			drop(file_guard);

			Ok(Arc::new(file))
		}
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> io::Result<()> {
		let path = self.traversal_path(components);

		let (cmd, rsp_payload_len) = ops::Unlink::create(path);
		let rsp = get_filesystem_driver()
			.ok_or(io::Error::ENOSYS)?
			.lock()
			.send_command(cmd, rsp_payload_len)?;
		trace!("unlink answer {rsp:?}");

		Ok(())
	}

	fn traverse_rmdir(&self, components: &mut Vec<&str>) -> io::Result<()> {
		let path = self.traversal_path(components);

		let (cmd, rsp_payload_len) = ops::Rmdir::create(path);
		let rsp = get_filesystem_driver()
			.ok_or(io::Error::ENOSYS)?
			.lock()
			.send_command(cmd, rsp_payload_len)?;
		trace!("rmdir answer {rsp:?}");

		Ok(())
	}

	fn traverse_mkdir(&self, components: &mut Vec<&str>, mode: AccessPermission) -> io::Result<()> {
		let path = self.traversal_path(components);
		let (cmd, rsp_payload_len) = ops::Mkdir::create(path, mode.bits());

		let rsp = get_filesystem_driver()
			.ok_or(io::Error::ENOSYS)?
			.lock()
			.send_command(cmd, rsp_payload_len)?;
		if rsp.headers.out_header.error == 0 {
			Ok(())
		} else {
			Err(io::Error::try_from(-rsp.headers.out_header.error).unwrap())
		}
	}
}

pub(crate) fn init() {
	debug!("Try to initialize fuse filesystem");

	if let Some(driver) = get_filesystem_driver() {
		let (cmd, rsp_payload_len) = ops::Init::create();
		let rsp = driver.lock().send_command(cmd, rsp_payload_len).unwrap();
		trace!("fuse init answer: {rsp:?}");

		let mount_point = driver.lock().get_mount_point();
		if mount_point == "/" {
			let fuse_nid = lookup(c"/".to_owned()).unwrap();
			// Opendir
			// Flag 0x10000 for O_DIRECTORY might not be necessary
			let (mut cmd, rsp_payload_len) = ops::Open::create(fuse_nid, 0x10000);
			cmd.headers.in_header.opcode = fuse_opcode::FUSE_OPENDIR as u32;
			let rsp = get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp_payload_len)
				.unwrap();
			let fuse_fh = rsp.headers.op_header.fh;

			// Linux seems to allocate a single page to store the dirfile
			let len = MAX_READ_LEN as u32;
			let mut offset: usize = 0;

			// read content of the directory
			let (mut cmd, rsp_payload_len) = ops::Read::create(fuse_nid, fuse_fh, len, 0);
			cmd.headers.in_header.opcode = fuse_opcode::FUSE_READDIR as u32;
			let rsp = get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp_payload_len)
				.unwrap();

			let len: usize = if rsp.headers.out_header.len as usize
				- mem::size_of::<fuse_out_header>()
				>= usize::try_from(len).unwrap()
			{
				len.try_into().unwrap()
			} else {
				(rsp.headers.out_header.len as usize) - mem::size_of::<fuse_out_header>()
			};

			assert!(
				len > core::mem::size_of::<fuse_dirent>(),
				"FUSE no new dirs"
			);

			let mut entries: Vec<String> = Vec::new();
			while (rsp.headers.out_header.len as usize) - offset
				> core::mem::size_of::<fuse_dirent>()
			{
				let dirent = unsafe {
					&*rsp
						.payload
						.as_ref()
						.unwrap()
						.as_ptr()
						.byte_add(offset)
						.cast::<fuse_dirent>()
				};

				offset += core::mem::size_of::<fuse_dirent>() + dirent.namelen as usize;
				// Align to dirent struct
				offset = ((offset) + U64_SIZE - 1) & (!(U64_SIZE - 1));

				let name: &'static [u8] = unsafe {
					core::slice::from_raw_parts(
						dirent.name.as_ptr().cast(),
						dirent.namelen.try_into().unwrap(),
					)
				};
				entries.push(unsafe { core::str::from_utf8_unchecked(name).to_string() });
			}

			let (cmd, rsp_payload_len) = ops::Release::create(fuse_nid, fuse_fh);
			get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp_payload_len)
				.unwrap();

			// remove predefined directories
			entries.retain(|x| x != ".");
			entries.retain(|x| x != "..");
			entries.retain(|x| x != "tmp");
			entries.retain(|x| x != "proc");
			warn!(
				"Fuse don't mount the host directories 'tmp' and 'proc' into the guest file system!"
			);

			for i in entries {
				let i_cstr = CString::new(i.clone()).unwrap();
				let (cmd, rsp_payload_len) = ops::Lookup::create(i_cstr);
				let rsp = get_filesystem_driver()
					.unwrap()
					.lock()
					.send_command(cmd, rsp_payload_len)
					.unwrap();

				assert_eq!(rsp.headers.out_header.error, 0);
				let entry_out =
					fuse_entry_out::ref_from_bytes(rsp.payload.as_ref().unwrap()).unwrap();
				let attr = entry_out.attr;
				let attr = FileAttr::from(attr);

				if attr.st_mode.contains(AccessPermission::S_IFDIR) {
					info!("Fuse mount {i} to /{i}");
					fs::FILESYSTEM
						.get()
						.unwrap()
						.mount(
							&("/".to_owned() + i.as_str()),
							Box::new(FuseDirectory::new(Some(i))),
						)
						.expect("Mount failed. Invalid mount_point?");
				} else {
					warn!("Fuse don't mount {i}. It isn't a directory!");
				}
			}
		} else {
			let mount_point = if mount_point.starts_with('/') {
				mount_point
			} else {
				"/".to_owned() + &mount_point
			};

			info!("Mounting virtio-fs at {mount_point}");
			fs::FILESYSTEM
				.get()
				.unwrap()
				.mount(mount_point.as_str(), Box::new(FuseDirectory::new(None)))
				.expect("Mount failed. Invalid mount_point?");
		}
	}
}
