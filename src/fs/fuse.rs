use alloc::alloc::{alloc, Layout};
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::Poll;

use async_lock::Mutex;
use async_trait::async_trait;

use crate::alloc::string::ToString;
use crate::arch;
#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio::get_filesystem_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_filesystem_driver;
use crate::drivers::virtio::virtqueue::error::VirtqError;
use crate::drivers::virtio::virtqueue::AsSliceU8;
use crate::executor::block_on;
use crate::fd::{IoError, PollEvent};
use crate::fs::{
	self, fuse_abi, AccessPermission, DirectoryEntry, FileAttr, NodeKind, ObjectInterface,
	OpenOption, SeekWhence, VfsNode,
};
use crate::time::{time_t, timespec};

// response out layout eg @ https://github.com/zargony/fuse-rs/blob/bf6d1cf03f3277e35b580f3c7b9999255d72ecf3/src/ll/request.rs#L44
// op in/out sizes/layout: https://github.com/hanwen/go-fuse/blob/204b45dba899dfa147235c255908236d5fde2d32/fuse/opcode.go#L439
// possible responses for command: qemu/tools/virtiofsd/fuse_lowlevel.h

const MAX_READ_LEN: usize = 1024 * 64;
const MAX_WRITE_LEN: usize = 1024 * 64;

const U64_SIZE: usize = ::core::mem::size_of::<u64>();

const S_IFLNK: u32 = 40960;
const S_IFMT: u32 = 61440;

pub(crate) trait FuseInterface {
	fn send_command<O: ops::Op>(
		&mut self,
		cmd: (Box<CmdHeader<O>>, Option<Box<[u8]>>),
		rsp: &mut Rsp<O>,
	) -> Result<(), VirtqError>;

	fn get_mount_point(&self) -> String;
}

pub(crate) mod ops {
	#![allow(clippy::type_complexity)]
	use alloc::boxed::Box;
	use alloc::ffi::CString;
	use core::mem::MaybeUninit;

	use super::{CmdHeader, Rsp};
	use crate::fd::PollEvent;
	use crate::fs::{fuse_abi, SeekWhence};

	pub(crate) trait Op {
		const OP_CODE: fuse_abi::Opcode;

		type InStruct: core::fmt::Debug;
		type InPayload: ?Sized;
		type OutStruct: core::fmt::Debug;
		type OutPayload: ?Sized;
	}

	#[derive(Debug)]
	pub(crate) struct Init;

	impl Op for Init {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Init;
		type InStruct = fuse_abi::InitIn;
		type InPayload = ();
		type OutStruct = fuse_abi::InitOut;
		type OutPayload = ();
	}

	impl Init {
		pub(crate) fn create() -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::new(
				fuse_abi::ROOT_ID,
				fuse_abi::InitIn {
					major: 7,
					minor: 31,
					max_readahead: 0,
					flags: 0,
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, None), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Create;

	impl Op for Create {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Create;
		type InStruct = fuse_abi::CreateIn;
		type InPayload = CString;
		type OutStruct = fuse_abi::CreateOut;
		type OutPayload = ();
	}

	impl Create {
		#[allow(clippy::self_named_constructors)]
		pub(crate) fn create(
			path: CString,
			flags: u32,
			mode: u32,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let path_bytes = path.into_bytes_with_nul().into_boxed_slice();
			let cmd = CmdHeader::<Self>::with_payload_size(
				fuse_abi::ROOT_ID,
				fuse_abi::CreateIn {
					flags,
					mode,
					..Default::default()
				},
				path_bytes.len(),
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, Some(path_bytes)), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Open;

	impl Op for Open {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Open;
		type InStruct = fuse_abi::OpenIn;
		type InPayload = ();
		type OutStruct = fuse_abi::OpenOut;
		type OutPayload = ();
	}

	impl Open {
		pub(crate) fn create(
			nid: u64,
			flags: u32,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::new(
				nid,
				fuse_abi::OpenIn {
					flags,
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, None), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Write;

	impl Op for Write {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Write;
		type InStruct = fuse_abi::WriteIn;
		type InPayload = [u8];
		type OutStruct = fuse_abi::WriteOut;
		type OutPayload = ();
	}

	impl Write {
		pub(crate) fn create(
			nid: u64,
			fh: u64,
			buf: Box<[u8]>,
			offset: u64,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::with_payload_size(
				nid,
				fuse_abi::WriteIn {
					fh,
					offset,
					size: buf.len().try_into().unwrap(),
					..Default::default()
				},
				buf.len(),
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, Some(buf)), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Read;

	impl Op for Read {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Read;
		type InStruct = fuse_abi::ReadIn;
		type InPayload = ();
		type OutStruct = fuse_abi::ReadOut;

		// Since at the time of writing MaybeUninit does not support DSTs as type parameters, we have to define `OutPayload` as [MaybeUninit<_>]
		// instead of a MaybeUninit<[_]>.
		type OutPayload = [MaybeUninit<u8>];
	}

	impl Read {
		pub(crate) fn create(
			nid: u64,
			fh: u64,
			size: u32,
			offset: u64,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::new(
				nid,
				fuse_abi::ReadIn {
					fh,
					offset,
					size,
					..Default::default()
				},
			);
			let rsp = unsafe { Rsp::<Self>::new_uninit(size.try_into().unwrap()) };

			((cmd, None), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Lseek;

	impl Op for Lseek {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Lseek;
		type InStruct = fuse_abi::LseekIn;
		type InPayload = ();
		type OutStruct = fuse_abi::LseekOut;
		type OutPayload = ();
	}

	impl Lseek {
		pub(crate) fn create(
			nid: u64,
			fh: u64,
			offset: isize,
			whence: SeekWhence,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::new(
				nid,
				fuse_abi::LseekIn {
					fh,
					offset: offset.try_into().unwrap(),
					whence: num::ToPrimitive::to_u32(&whence).unwrap(),
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, None), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Readlink;

	impl Op for Readlink {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Readlink;
		type InStruct = fuse_abi::ReadlinkIn;
		type InPayload = ();
		type OutStruct = fuse_abi::ReadlinkOut;

		// Since at the time of writing MaybeUninit does not support DSTs as type parameters, we have to define `OutPayload` as [MaybeUninit<_>]
		// instead of a MaybeUninit<[_]>.
		type OutPayload = [MaybeUninit<u8>];
	}

	impl Readlink {
		pub(crate) fn create(
			nid: u64,
			size: u32,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::new(nid, fuse_abi::ReadlinkIn {});
			let rsp = unsafe { Rsp::<Self>::new_uninit(size.try_into().unwrap()) };

			((cmd, None), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Release;

	impl Op for Release {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Release;
		type InStruct = fuse_abi::ReleaseIn;
		type InPayload = ();
		type OutStruct = fuse_abi::ReleaseOut;
		type OutPayload = ();
	}

	impl Release {
		pub(crate) fn create(
			nid: u64,
			fh: u64,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::new(
				nid,
				fuse_abi::ReleaseIn {
					fh,
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, None), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Poll;

	impl Op for Poll {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Poll;
		type InStruct = fuse_abi::PollIn;
		type InPayload = ();
		type OutStruct = fuse_abi::PollOut;
		type OutPayload = ();
	}

	impl Poll {
		pub(crate) fn create(
			nid: u64,
			fh: u64,
			kh: u64,
			event: PollEvent,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let cmd = CmdHeader::<Self>::new(
				nid,
				fuse_abi::PollIn {
					fh,
					kh,
					events: event.bits() as u32,
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, None), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Mkdir;

	impl Op for Mkdir {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Mkdir;
		type InStruct = fuse_abi::MkdirIn;
		type InPayload = CString;
		type OutStruct = fuse_abi::EntryOut;
		type OutPayload = ();
	}

	impl Mkdir {
		pub(crate) fn create(
			path: CString,
			mode: u32,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let path_bytes = path.into_bytes_with_nul().into_boxed_slice();
			let cmd = CmdHeader::<Self>::with_payload_size(
				fuse_abi::ROOT_ID,
				fuse_abi::MkdirIn {
					mode,
					..Default::default()
				},
				path_bytes.len(),
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, Some(path_bytes)), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Unlink;

	impl Op for Unlink {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Unlink;
		type InStruct = fuse_abi::UnlinkIn;
		type InPayload = CString;
		type OutStruct = fuse_abi::UnlinkOut;
		type OutPayload = ();
	}

	impl Unlink {
		pub(crate) fn create(
			name: CString,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let name_bytes = name.into_bytes_with_nul().into_boxed_slice();
			let cmd = CmdHeader::<Self>::with_payload_size(
				fuse_abi::ROOT_ID,
				fuse_abi::UnlinkIn {},
				name_bytes.len(),
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, Some(name_bytes)), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Rmdir;

	impl Op for Rmdir {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Rmdir;
		type InStruct = fuse_abi::RmdirIn;
		type InPayload = CString;
		type OutStruct = fuse_abi::RmdirOut;
		type OutPayload = ();
	}

	impl Rmdir {
		pub(crate) fn create(
			name: CString,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let name_bytes = name.into_bytes_with_nul().into_boxed_slice();
			let cmd = CmdHeader::<Self>::with_payload_size(
				fuse_abi::ROOT_ID,
				fuse_abi::RmdirIn {},
				name_bytes.len(),
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, Some(name_bytes)), rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Lookup;

	impl Op for Lookup {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Lookup;
		type InStruct = fuse_abi::LookupIn;
		type InPayload = CString;
		type OutStruct = fuse_abi::EntryOut;
		type OutPayload = ();
	}

	impl Lookup {
		pub(crate) fn create(
			name: CString,
		) -> ((Box<CmdHeader<Self>>, Option<Box<[u8]>>), Box<Rsp<Self>>) {
			let name_bytes = name.into_bytes_with_nul().into_boxed_slice();
			let cmd = CmdHeader::<Self>::with_payload_size(
				fuse_abi::ROOT_ID,
				fuse_abi::LookupIn {},
				name_bytes.len(),
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			((cmd, Some(name_bytes)), rsp)
		}
	}
}

impl From<fuse_abi::Attr> for FileAttr {
	fn from(attr: fuse_abi::Attr) -> FileAttr {
		FileAttr {
			st_ino: attr.ino,
			st_nlink: attr.nlink as u64,
			st_mode: AccessPermission::from_bits_retain(attr.mode),
			st_uid: attr.uid,
			st_gid: attr.gid,
			st_rdev: attr.rdev as u64,
			st_size: attr.size,
			st_blksize: attr.blksize as i64,
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
	pub in_header: fuse_abi::InHeader,
	op_header: O::InStruct,
}

impl<O: ops::Op> CmdHeader<O>
where
	O: ops::Op<InPayload = ()>,
{
	fn new(nodeid: u64, op_header: O::InStruct) -> Box<Self> {
		Self::with_payload_size(nodeid, op_header, 0)
	}
}

impl<O: ops::Op> CmdHeader<O> {
	fn with_payload_size(nodeid: u64, op_header: O::InStruct, len: usize) -> Box<CmdHeader<O>> {
		Box::new(CmdHeader {
			in_header: fuse_abi::InHeader {
				// The length we need the provide in the header is not the same as the size of the struct because of padding, so we need to calculate it manually.
				len: (core::mem::size_of::<fuse_abi::InHeader>()
					+ core::mem::size_of::<O::InStruct>()
					+ len)
					.try_into()
					.expect("The command is too large"),
				opcode: O::OP_CODE as u32,
				nodeid,
				unique: 1,
				..Default::default()
			},
			op_header,
		})
	}
}

impl<O: ops::Op> AsSliceU8 for CmdHeader<O> {}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct Rsp<O: ops::Op> {
	out_header: MaybeUninit<fuse_abi::OutHeader>,
	op_header: MaybeUninit<O::OutStruct>,
	payload: O::OutPayload,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct PayloadlessRsp<O: ops::Op> {
	out_header: MaybeUninit<fuse_abi::OutHeader>,
	op_header: MaybeUninit<O::OutStruct>,
	payload: (),
}

// Since we don't bother with initializing the len field, we use the default len implementation.
impl<O: ops::Op> AsSliceU8 for Rsp<O> {}

impl<O: ops::Op> Rsp<O>
where
	O: ops::Op<OutPayload = [MaybeUninit<u8>]>,
{
	unsafe fn new_uninit(len: usize) -> Box<Self> {
		unsafe {
			Box::from_raw(core::ptr::slice_from_raw_parts_mut(
				alloc(
					Layout::new::<PayloadlessRsp<O>>()
						.extend(Layout::array::<u8>(len).expect("The length is too much."))
						.expect("The layout size overflowed.")
						.0 // We don't need the offset of `data_header` inside the type (the second element of the tuple)
						.pad_to_align(),
				),
				len,
			) as *mut Rsp<O>)
		}
	}
}

fn lookup(name: CString) -> Option<u64> {
	let (cmd, mut rsp) = ops::Lookup::create(name);
	get_filesystem_driver()
		.unwrap()
		.lock()
		.send_command(cmd, rsp.as_mut())
		.ok()?;
	if unsafe { rsp.out_header.assume_init_ref().error } == 0 {
		Some(unsafe { rsp.op_header.assume_init_ref().nodeid })
	} else {
		None
	}
}

fn readlink(nid: u64) -> Result<String, IoError> {
	let len = MAX_READ_LEN as u32;
	let (cmd, mut rsp) = ops::Readlink::create(nid, len);
	get_filesystem_driver()
		.unwrap()
		.lock()
		.send_command(cmd, rsp.as_mut())?;
	let len: usize = if unsafe { rsp.out_header.assume_init_ref().len } as usize
		- ::core::mem::size_of::<fuse_abi::OutHeader>()
		- ::core::mem::size_of::<fuse_abi::ReadlinkOut>()
		>= len.try_into().unwrap()
	{
		len.try_into().unwrap()
	} else {
		(unsafe { rsp.out_header.assume_init_ref().len } as usize)
			- ::core::mem::size_of::<fuse_abi::OutHeader>()
			- ::core::mem::size_of::<fuse_abi::ReadlinkOut>()
	};

	Ok(String::from_utf8(unsafe {
		MaybeUninit::slice_assume_init_ref(&rsp.payload[..len]).to_vec()
	})
	.unwrap())
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

	async fn poll(&self, events: PollEvent) -> Result<PollEvent, IoError> {
		static KH: AtomicU64 = AtomicU64::new(0);
		let kh = KH.fetch_add(1, Ordering::SeqCst);

		future::poll_fn(|cx| {
			if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
				let (cmd, mut rsp) = ops::Poll::create(nid, fh, kh, events);
				get_filesystem_driver()
					.ok_or(IoError::ENOSYS)?
					.lock()
					.send_command(cmd, rsp.as_mut())?;

				if unsafe { rsp.out_header.assume_init_ref().error } < 0 {
					Poll::Ready(Err(IoError::EIO))
				} else {
					let revents = unsafe {
						PollEvent::from_bits(
							i16::try_from(rsp.op_header.assume_init_ref().revents).unwrap(),
						)
						.unwrap()
					};
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

	fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
		let mut len = buf.len();
		if len > MAX_READ_LEN {
			debug!("Reading longer than max_read_len: {}", len);
			len = MAX_READ_LEN;
		}
		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, mut rsp) =
				ops::Read::create(nid, fh, len.try_into().unwrap(), self.offset as u64);
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command(cmd, rsp.as_mut())?;
			let len: usize = if (unsafe { rsp.out_header.assume_init_ref().len } as usize)
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
				>= len
			{
				len
			} else {
				(unsafe { rsp.out_header.assume_init_ref().len } as usize)
					- ::core::mem::size_of::<fuse_abi::OutHeader>()
					- ::core::mem::size_of::<fuse_abi::ReadOut>()
			};
			self.offset += len;

			buf[..len].copy_from_slice(unsafe {
				MaybeUninit::slice_assume_init_ref(&rsp.payload[..len])
			});

			Ok(len)
		} else {
			debug!("File not open, cannot read!");
			Err(IoError::ENOENT)
		}
	}

	fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
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
			let (cmd, mut rsp) = ops::Write::create(nid, fh, truncated_buf, self.offset as u64);
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command(cmd, rsp.as_mut())?;

			if unsafe { rsp.out_header.assume_init_ref().error } < 0 {
				return Err(IoError::EIO);
			}

			let rsp_size = unsafe { rsp.op_header.assume_init_ref().size };
			let rsp_len: usize = if rsp_size > truncated_len.try_into().unwrap() {
				truncated_len
			} else {
				rsp_size.try_into().unwrap()
			};
			self.offset += rsp_len;
			Ok(rsp_len)
		} else {
			warn!("File not open, cannot read!");
			Err(IoError::ENOENT)
		}
	}

	fn lseek(&mut self, offset: isize, whence: SeekWhence) -> Result<isize, IoError> {
		debug!("FUSE lseek");

		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, mut rsp) = ops::Lseek::create(nid, fh, offset, whence);
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command(cmd, rsp.as_mut())?;

			if unsafe { rsp.out_header.assume_init_ref().error } < 0 {
				return Err(IoError::EIO);
			}

			let rsp_offset = unsafe { rsp.op_header.assume_init_ref().offset };

			Ok(rsp_offset.try_into().unwrap())
		} else {
			Err(IoError::EIO)
		}
	}
}

impl Drop for FuseFileHandleInner {
	fn drop(&mut self) {
		if self.fuse_nid.is_some() && self.fuse_fh.is_some() {
			let (cmd, mut rsp) =
				ops::Release::create(self.fuse_nid.unwrap(), self.fuse_fh.unwrap());
			get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp.as_mut())
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
	async fn poll(&self, event: PollEvent) -> Result<PollEvent, IoError> {
		self.0.lock().await.poll(event).await
	}

	async fn async_read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
		self.0.lock().await.read(buf)
	}

	async fn async_write(&self, buf: &[u8]) -> Result<usize, IoError> {
		self.0.lock().await.write(buf)
	}

	fn lseek(&self, offset: isize, whence: SeekWhence) -> Result<isize, IoError> {
		block_on(async { self.0.lock().await.lseek(offset, whence) }, None)
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
	fn readdir(&self) -> Result<Vec<DirectoryEntry>, IoError> {
		let path: CString = if let Some(name) = &self.name {
			CString::new("/".to_string() + name).unwrap()
		} else {
			CString::new("/".to_string()).unwrap()
		};

		debug!("FUSE opendir: {path:#?}");

		let fuse_nid = lookup(path.clone()).ok_or(IoError::ENOENT)?;

		// Opendir
		// Flag 0x10000 for O_DIRECTORY might not be necessary
		let (mut cmd, mut rsp) = ops::Open::create(fuse_nid, 0x10000);
		cmd.0.in_header.opcode = fuse_abi::Opcode::Opendir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd, rsp.as_mut())?;
		let fuse_fh = unsafe { rsp.op_header.assume_init_ref().fh };

		debug!("FUSE readdir: {path:#?}");

		// Linux seems to allocate a single page to store the dirfile
		let len = MAX_READ_LEN as u32;
		let mut offset: usize = 0;

		// read content of the directory
		let (mut cmd, mut rsp) = ops::Read::create(fuse_nid, fuse_fh, len, 0);
		cmd.0.in_header.opcode = fuse_abi::Opcode::Readdir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd, rsp.as_mut())?;

		let len: usize = if unsafe { rsp.out_header.assume_init_ref().len } as usize
			- ::core::mem::size_of::<fuse_abi::OutHeader>()
			- ::core::mem::size_of::<fuse_abi::ReadOut>()
			>= len.try_into().unwrap()
		{
			len.try_into().unwrap()
		} else {
			(unsafe { rsp.out_header.assume_init_ref().len } as usize)
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
		};

		if len <= core::mem::size_of::<fuse_abi::Dirent>() {
			debug!("FUSE no new dirs");
			return Err(IoError::ENOENT);
		}

		let mut entries: Vec<DirectoryEntry> = Vec::new();
		while (unsafe { rsp.out_header.assume_init_ref().len } as usize) - offset
			> core::mem::size_of::<fuse_abi::Dirent>()
		{
			let dirent =
				unsafe { &*(rsp.payload.as_ptr().byte_add(offset) as *const fuse_abi::Dirent) };

			offset += core::mem::size_of::<fuse_abi::Dirent>() + dirent.d_namelen as usize;
			// Align to dirent struct
			offset = ((offset) + U64_SIZE - 1) & (!(U64_SIZE - 1));

			let name: &'static [u8] = unsafe {
				core::slice::from_raw_parts(
					dirent.d_name.as_ptr(),
					dirent.d_namelen.try_into().unwrap(),
				)
			};
			entries.push(DirectoryEntry::new(unsafe {
				core::str::from_utf8_unchecked(name).to_string()
			}));
		}

		let (cmd, mut rsp) = ops::Release::create(fuse_nid, fuse_fh);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp.as_mut())?;

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

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		Ok(self.attr)
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(FuseDirectoryHandle::new(self.prefix.clone())))
	}

	fn traverse_readdir(&self, components: &mut Vec<&str>) -> Result<Vec<DirectoryEntry>, IoError> {
		let path = self.traversal_path(components);

		debug!("FUSE opendir: {path:#?}");

		let fuse_nid = lookup(path.clone()).ok_or(IoError::ENOENT)?;

		// Opendir
		// Flag 0x10000 for O_DIRECTORY might not be necessary
		let (mut cmd, mut rsp) = ops::Open::create(fuse_nid, 0x10000);
		cmd.0.in_header.opcode = fuse_abi::Opcode::Opendir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd, rsp.as_mut())?;
		let fuse_fh = unsafe { rsp.op_header.assume_init_ref().fh };

		debug!("FUSE readdir: {path:#?}");

		// Linux seems to allocate a single page to store the dirfile
		let len = MAX_READ_LEN as u32;
		let mut offset: usize = 0;

		// read content of the directory
		let (mut cmd, mut rsp) = ops::Read::create(fuse_nid, fuse_fh, len, 0);
		cmd.0.in_header.opcode = fuse_abi::Opcode::Readdir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd, rsp.as_mut())?;

		let len: usize = if unsafe { rsp.out_header.assume_init_ref().len } as usize
			- ::core::mem::size_of::<fuse_abi::OutHeader>()
			- ::core::mem::size_of::<fuse_abi::ReadOut>()
			>= len.try_into().unwrap()
		{
			len.try_into().unwrap()
		} else {
			(unsafe { rsp.out_header.assume_init_ref().len } as usize)
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
		};

		if len <= core::mem::size_of::<fuse_abi::Dirent>() {
			debug!("FUSE no new dirs");
			return Err(IoError::ENOENT);
		}

		let mut entries: Vec<DirectoryEntry> = Vec::new();
		while (unsafe { rsp.out_header.assume_init_ref().len } as usize) - offset
			> core::mem::size_of::<fuse_abi::Dirent>()
		{
			let dirent =
				unsafe { &*(rsp.payload.as_ptr().byte_add(offset) as *const fuse_abi::Dirent) };

			offset += core::mem::size_of::<fuse_abi::Dirent>() + dirent.d_namelen as usize;
			// Align to dirent struct
			offset = ((offset) + U64_SIZE - 1) & (!(U64_SIZE - 1));

			let name: &'static [u8] = unsafe {
				core::slice::from_raw_parts(
					dirent.d_name.as_ptr(),
					dirent.d_namelen.try_into().unwrap(),
				)
			};
			entries.push(DirectoryEntry::new(unsafe {
				core::str::from_utf8_unchecked(name).to_string()
			}));
		}

		let (cmd, mut rsp) = ops::Release::create(fuse_nid, fuse_fh);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp.as_mut())?;

		Ok(entries)
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		let path = self.traversal_path(components);

		debug!("FUSE stat: {path:#?}");

		// Is there a better way to implement this?
		let (cmd, mut rsp) = ops::Lookup::create(path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp.as_mut())?;

		if unsafe { rsp.out_header.assume_init_ref().error } != 0 {
			// TODO: Correct error handling
			return Err(IoError::EIO);
		}

		let rsp = unsafe { rsp.op_header.assume_init() };
		let attr = rsp.attr;

		if attr.mode & S_IFMT != S_IFLNK {
			Ok(FileAttr::from(attr))
		} else {
			let path = readlink(rsp.nodeid)?;
			let mut components: Vec<&str> = path.split('/').collect();
			self.traverse_stat(&mut components)
		}
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		let path = self.traversal_path(components);

		debug!("FUSE lstat: {path:#?}");

		let (cmd, mut rsp) = ops::Lookup::create(path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp.as_mut())?;

		let attr = unsafe { rsp.op_header.assume_init().attr };
		Ok(FileAttr::from(attr))
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		let path = self.traversal_path(components);

		debug!("FUSE lstat: {path:#?}");

		let (cmd, mut rsp) = ops::Lookup::create(path.clone());
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd, rsp.as_mut())?;

		let attr = unsafe { FileAttr::from(rsp.op_header.assume_init().attr) };
		let is_dir = attr.st_mode.contains(AccessPermission::S_IFDIR);

		debug!("FUSE open: {path:#?}, {opt:?} {mode:?}");

		if is_dir {
			let mut path = path.into_string().unwrap();
			path.remove(0);
			Ok(Arc::new(FuseDirectoryHandle::new(Some(path))))
		} else {
			if opt.contains(OpenOption::O_DIRECTORY) {
				return Err(IoError::ENOTDIR);
			}

			let file = FuseFileHandle::new();

			// 1.FUSE_INIT to create session
			// Already done
			let mut file_guard = block_on(async { Ok(file.0.lock().await) }, None)?;

			// Differentiate between opening and creating new file, since fuse does not support O_CREAT on open.
			if !opt.contains(OpenOption::O_CREAT) {
				// 2.FUSE_LOOKUP(FUSE_ROOT_ID, “foo”) -> nodeid
				file_guard.fuse_nid = lookup(path);

				if file_guard.fuse_nid.is_none() {
					warn!("Fuse lookup seems to have failed!");
					return Err(IoError::ENOENT);
				}

				// 3.FUSE_OPEN(nodeid, O_RDONLY) -> fh
				let (cmd, mut rsp) =
					ops::Open::create(file_guard.fuse_nid.unwrap(), opt.bits().try_into().unwrap());
				get_filesystem_driver()
					.ok_or(IoError::ENOSYS)?
					.lock()
					.send_command(cmd, rsp.as_mut())?;
				file_guard.fuse_fh = Some(unsafe { rsp.op_header.assume_init_ref().fh });
			} else {
				// Create file (opens implicitly, returns results from both lookup and open calls)
				let (cmd, mut rsp) =
					ops::Create::create(path, opt.bits().try_into().unwrap(), mode.bits());
				get_filesystem_driver()
					.ok_or(IoError::ENOSYS)?
					.lock()
					.send_command(cmd, rsp.as_mut())?;

				let inner = unsafe { rsp.op_header.assume_init() };
				file_guard.fuse_nid = Some(inner.entry.nodeid);
				file_guard.fuse_fh = Some(inner.open.fh);
			}

			drop(file_guard);

			Ok(Arc::new(file))
		}
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> core::result::Result<(), IoError> {
		let path = self.traversal_path(components);

		let (cmd, mut rsp) = ops::Unlink::create(path);
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd, rsp.as_mut())?;
		trace!("unlink answer {:?}", rsp);

		Ok(())
	}

	fn traverse_rmdir(&self, components: &mut Vec<&str>) -> core::result::Result<(), IoError> {
		let path = self.traversal_path(components);

		let (cmd, mut rsp) = ops::Rmdir::create(path);
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd, rsp.as_mut())?;
		trace!("rmdir answer {:?}", rsp);

		Ok(())
	}

	fn traverse_mkdir(
		&self,
		components: &mut Vec<&str>,
		mode: AccessPermission,
	) -> Result<(), IoError> {
		let path = self.traversal_path(components);
		let (cmd, mut rsp) = ops::Mkdir::create(path, mode.bits());

		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd, rsp.as_mut())?;
		if unsafe { rsp.out_header.assume_init_ref().error } == 0 {
			Ok(())
		} else {
			Err(
				num::FromPrimitive::from_i32(unsafe { rsp.out_header.assume_init_ref().error })
					.unwrap(),
			)
		}
	}
}

pub(crate) fn init() {
	debug!("Try to initialize fuse filesystem");

	if let Some(driver) = get_filesystem_driver() {
		let (cmd, mut rsp) = ops::Init::create();
		driver.lock().send_command(cmd, rsp.as_mut()).unwrap();
		trace!("fuse init answer: {:?}", rsp);

		let mount_point = driver.lock().get_mount_point().to_string();
		if mount_point == "/" {
			let fuse_nid = lookup(c"/".to_owned()).unwrap();
			// Opendir
			// Flag 0x10000 for O_DIRECTORY might not be necessary
			let (mut cmd, mut rsp) = ops::Open::create(fuse_nid, 0x10000);
			cmd.0.in_header.opcode = fuse_abi::Opcode::Opendir as u32;
			get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp.as_mut())
				.unwrap();
			let fuse_fh = unsafe { rsp.op_header.assume_init_ref().fh };

			// Linux seems to allocate a single page to store the dirfile
			let len = MAX_READ_LEN as u32;
			let mut offset: usize = 0;

			// read content of the directory
			let (mut cmd, mut rsp) = ops::Read::create(fuse_nid, fuse_fh, len, 0);
			cmd.0.in_header.opcode = fuse_abi::Opcode::Readdir as u32;
			get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp.as_mut())
				.unwrap();

			let len: usize = if unsafe { rsp.out_header.assume_init_ref().len } as usize
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
				>= len.try_into().unwrap()
			{
				len.try_into().unwrap()
			} else {
				(unsafe { rsp.out_header.assume_init_ref().len } as usize)
					- ::core::mem::size_of::<fuse_abi::OutHeader>()
					- ::core::mem::size_of::<fuse_abi::ReadOut>()
			};

			if len <= core::mem::size_of::<fuse_abi::Dirent>() {
				panic!("FUSE no new dirs");
			}

			let mut entries: Vec<String> = Vec::new();
			while (unsafe { rsp.out_header.assume_init_ref().len } as usize) - offset
				> core::mem::size_of::<fuse_abi::Dirent>()
			{
				let dirent =
					unsafe { &*(rsp.payload.as_ptr().byte_add(offset) as *const fuse_abi::Dirent) };

				offset += core::mem::size_of::<fuse_abi::Dirent>() + dirent.d_namelen as usize;
				// Allign to dirent struct
				offset = ((offset) + U64_SIZE - 1) & (!(U64_SIZE - 1));

				let name: &'static [u8] = unsafe {
					core::slice::from_raw_parts(
						dirent.d_name.as_ptr(),
						dirent.d_namelen.try_into().unwrap(),
					)
				};
				entries.push(unsafe { core::str::from_utf8_unchecked(name).to_string() });
			}

			let (cmd, mut rsp) = ops::Release::create(fuse_nid, fuse_fh);
			get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command(cmd, rsp.as_mut())
				.unwrap();

			// remove predefined directories
			entries.retain(|x| x != ".");
			entries.retain(|x| x != "..");
			entries.retain(|x| x != "tmp");
			entries.retain(|x| x != "proc");
			warn!("Fuse don't mount the host directories 'tmp' and 'proc' into the guest file system!");

			for i in entries {
				let i_cstr = CString::new(i.clone()).unwrap();
				let (cmd, mut rsp) = ops::Lookup::create(i_cstr);
				get_filesystem_driver()
					.unwrap()
					.lock()
					.send_command(cmd, rsp.as_mut())
					.unwrap();

				let attr = unsafe { rsp.op_header.assume_init().attr };
				let attr = FileAttr::from(attr);

				if attr.st_mode.contains(AccessPermission::S_IFDIR) {
					info!("Fuse mount {} to /{}", i, i);
					fs::FILESYSTEM
						.get()
						.unwrap()
						.mount(
							&("/".to_owned() + i.as_str()),
							Box::new(FuseDirectory::new(Some(i))),
						)
						.expect("Mount failed. Invalid mount_point?");
				} else {
					warn!("Fuse don't mount {}. It isn't a directory!", i);
				}
			}
		} else {
			let mount_point = if mount_point.starts_with('/') {
				mount_point
			} else {
				"/".to_owned() + &mount_point
			};

			info!("Mounting virtio-fs at {}", mount_point);
			fs::FILESYSTEM
				.get()
				.unwrap()
				.mount(mount_point.as_str(), Box::new(FuseDirectory::new(None)))
				.expect("Mount failed. Invalid mount_point?");
		}
	}
}
