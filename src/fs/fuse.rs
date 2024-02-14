use alloc::alloc::{alloc, Layout};
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::Poll;
use core::{future, u32, u8};

use async_lock::Mutex;
use async_trait::async_trait;

use crate::alloc::string::ToString;
#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio::get_filesystem_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_filesystem_driver;
use crate::drivers::virtio::virtqueue::AsSliceU8;
use crate::executor::block_on;
use crate::fd::{IoError, PollEvent};
use crate::fs::{
	self, fuse_abi, AccessPermission, DirectoryEntry, FileAttr, NodeKind, ObjectInterface,
	OpenOption, SeekWhence, VfsNode,
};

// response out layout eg @ https://github.com/zargony/fuse-rs/blob/bf6d1cf03f3277e35b580f3c7b9999255d72ecf3/src/ll/request.rs#L44
// op in/out sizes/layout: https://github.com/hanwen/go-fuse/blob/204b45dba899dfa147235c255908236d5fde2d32/fuse/opcode.go#L439
// possible responses for command: qemu/tools/virtiofsd/fuse_lowlevel.h

const MAX_READ_LEN: usize = 1024 * 64;
const MAX_WRITE_LEN: usize = 1024 * 64;

const U64_SIZE: usize = ::core::mem::size_of::<u64>();

const S_IFLNK: u32 = 40960;
const S_IFMT: u32 = 61440;

pub(crate) trait FuseInterface {
	fn send_command<O: ops::Op>(&mut self, cmd: &Cmd<O>, rsp: &mut Rsp<O>);

	fn get_mount_point(&self) -> String;
}

pub(crate) mod ops {
	use alloc::boxed::Box;
	use core::ffi::CStr;
	use core::mem::MaybeUninit;

	use super::{Cmd, Rsp};
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
		pub(crate) fn create() -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::new(
				fuse_abi::ROOT_ID,
				fuse_abi::InitIn {
					major: 7,
					minor: 31,
					max_readahead: 0,
					flags: 0,
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Create;

	impl Op for Create {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Create;
		type InStruct = fuse_abi::CreateIn;
		type InPayload = CStr;
		type OutStruct = fuse_abi::CreateOut;
		type OutPayload = ();
	}

	impl Create {
		#[allow(clippy::self_named_constructors)]
		pub(crate) fn create(
			path: &str,
			flags: u32,
			mode: u32,
		) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::from_str(
				fuse_abi::ROOT_ID,
				fuse_abi::CreateIn {
					flags,
					mode,
					..Default::default()
				},
				path,
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
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
		pub(crate) fn create(nid: u64, flags: u32) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::new(
				nid,
				fuse_abi::OpenIn {
					flags,
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
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
			buf: &[u8],
			offset: u64,
		) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::from_array(
				nid,
				fuse_abi::WriteIn {
					fh,
					offset,
					size: buf.len().try_into().unwrap(),
					..Default::default()
				},
				buf,
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
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
		) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::new(
				nid,
				fuse_abi::ReadIn {
					fh,
					offset,
					size,
					..Default::default()
				},
			);
			let rsp = unsafe { Rsp::<Self>::new_uninit(size.try_into().unwrap()) };

			(cmd, rsp)
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
		) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::new(
				nid,
				fuse_abi::LseekIn {
					fh,
					offset: offset.try_into().unwrap(),
					whence: num::ToPrimitive::to_u32(&whence).unwrap(),
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
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
		pub(crate) fn create(nid: u64, size: u32) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::new(nid, fuse_abi::ReadlinkIn {});
			let rsp = unsafe { Rsp::<Self>::new_uninit(size.try_into().unwrap()) };

			(cmd, rsp)
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
		pub(crate) fn create(nid: u64, fh: u64) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::new(
				nid,
				fuse_abi::ReleaseIn {
					fh,
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
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
		) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::new(
				nid,
				fuse_abi::PollIn {
					fh,
					kh,
					events: event.bits() as u32,
					..Default::default()
				},
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Mkdir;

	impl Op for Mkdir {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Mkdir;
		type InStruct = fuse_abi::MkdirIn;
		type InPayload = CStr;
		type OutStruct = fuse_abi::EntryOut;
		type OutPayload = ();
	}

	impl Mkdir {
		pub(crate) fn create(path: &str, mode: u32) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::from_str(
				fuse_abi::ROOT_ID,
				fuse_abi::MkdirIn {
					mode,
					..Default::default()
				},
				path,
			);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Unlink;

	impl Op for Unlink {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Unlink;
		type InStruct = fuse_abi::UnlinkIn;
		type InPayload = CStr;
		type OutStruct = fuse_abi::UnlinkOut;
		type OutPayload = ();
	}

	impl Unlink {
		pub(crate) fn create(name: &str) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::from_str(fuse_abi::ROOT_ID, fuse_abi::UnlinkIn {}, name);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Rmdir;

	impl Op for Rmdir {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Rmdir;
		type InStruct = fuse_abi::RmdirIn;
		type InPayload = CStr;
		type OutStruct = fuse_abi::RmdirOut;
		type OutPayload = ();
	}

	impl Rmdir {
		pub(crate) fn create(name: &str) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::from_str(fuse_abi::ROOT_ID, fuse_abi::RmdirIn {}, name);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
		}
	}

	#[derive(Debug)]
	pub(crate) struct Lookup;

	impl Op for Lookup {
		const OP_CODE: fuse_abi::Opcode = fuse_abi::Opcode::Lookup;
		type InStruct = fuse_abi::LookupIn;
		type InPayload = CStr;
		type OutStruct = fuse_abi::EntryOut;
		type OutPayload = ();
	}

	impl Lookup {
		pub(crate) fn create(name: &str) -> (Box<Cmd<Self>>, Box<Rsp<Self>>) {
			let cmd = Cmd::<Self>::from_str(fuse_abi::ROOT_ID, fuse_abi::LookupIn {}, name);
			let rsp = unsafe { Box::new_uninit().assume_init() };

			(cmd, rsp)
		}
	}
}

impl From<fuse_abi::Attr> for FileAttr {
	fn from(attr: fuse_abi::Attr) -> FileAttr {
		FileAttr {
			st_ino: attr.ino,
			st_nlink: attr.nlink as u64,
			st_mode: AccessPermission::from_bits(attr.mode).unwrap(),
			st_uid: attr.uid,
			st_gid: attr.gid,
			st_rdev: attr.rdev as u64,
			st_size: attr.size,
			st_blksize: attr.blksize as i64,
			st_blocks: attr.blocks.try_into().unwrap(),
			st_atime: attr.atime,
			st_atime_nsec: attr.atimensec as u64,
			st_mtime: attr.mtime,
			st_mtime_nsec: attr.atimensec as u64,
			st_ctime: attr.ctime,
			st_ctime_nsec: attr.ctimensec as u64,
			..Default::default()
		}
	}
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct Cmd<O: ops::Op> {
	in_header: fuse_abi::InHeader,
	op_header: O::InStruct,
	payload: O::InPayload,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct UninitCmd<O: ops::Op> {
	in_header: MaybeUninit<fuse_abi::InHeader>,
	op_header: MaybeUninit<O::InStruct>,
	payload: [MaybeUninit<u8>],
}

// We use this struct to obtain the layout of the type without the payload.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct PayloadlessCmd<O: ops::Op> {
	in_header: MaybeUninit<fuse_abi::InHeader>,
	op_header: MaybeUninit<O::InStruct>,
	payload: (),
}

impl<O: ops::Op> Cmd<O>
where
	O: ops::Op<InPayload = ()>,
{
	fn new(nodeid: u64, op_header: O::InStruct) -> Box<Self> {
		Box::new(Cmd {
			in_header: fuse_abi::InHeader {
				len: Layout::new::<Self>().size() as u32,
				opcode: O::OP_CODE as u32,
				nodeid,
				unique: 1,
				..Default::default()
			},
			op_header,
			payload: (),
		})
	}
}

impl<O: ops::Op> Cmd<O> {
	fn with_capacity(nodeid: u64, op_header: O::InStruct, len: usize) -> Box<UninitCmd<O>> {
		let mut cmd = unsafe { Self::new_uninit(len) };
		cmd.in_header = MaybeUninit::new(fuse_abi::InHeader {
			len: core::mem::size_of_val(cmd.as_ref())
				.try_into()
				.expect("The command is too large"),
			opcode: O::OP_CODE as u32,
			nodeid,
			unique: 1,
			..Default::default()
		});
		cmd.op_header = MaybeUninit::new(op_header);
		cmd
	}
}

impl<O: ops::Op> Cmd<O>
where
	O: ops::Op<InPayload = [u8]>,
{
	fn from_array(nodeid: u64, op_header: O::InStruct, data: &[u8]) -> Box<Cmd<O>> {
		let mut cmd = Self::with_capacity(nodeid, op_header, data.len());
		MaybeUninit::write_slice(&mut cmd.payload, data);
		unsafe { core::intrinsics::transmute(cmd) }
	}
}

impl<O: ops::Op> Cmd<O>
where
	O: ops::Op<InPayload = CStr>,
{
	fn from_str(nodeid: u64, op_header: O::InStruct, str: &str) -> Box<Cmd<O>> {
		let str_bytes = str.as_bytes();
		// Plus one for the NUL terminator
		let mut cmd = Self::with_capacity(nodeid, op_header, str_bytes.len() + 1);
		MaybeUninit::write_slice(&mut cmd.payload[..str_bytes.len()], str_bytes);
		cmd.payload[str_bytes.len()] = MaybeUninit::new(b'\0');
		unsafe { core::intrinsics::transmute(cmd) }
	}
}

impl<O: ops::Op> AsSliceU8 for Cmd<O> {
	fn len(&self) -> usize {
		self.in_header.len.try_into().unwrap()
	}
}

impl<O: ops::Op> Cmd<O> {
	// MaybeUninit does not accept DSTs as type parameter
	unsafe fn new_uninit(len: usize) -> Box<UninitCmd<O>> {
		unsafe {
			Box::from_raw(core::ptr::slice_from_raw_parts_mut(
				alloc(
					Layout::new::<PayloadlessCmd<O>>()
						.extend(Layout::array::<u8>(len).expect("The length is too much."))
						.expect("The layout size overflowed.")
						.0 // We don't need the offset of `data_header` inside the type (the second element of the tuple)
						.pad_to_align(),
				),
				0,
			) as *mut UninitCmd<O>)
		}
	}
}

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
				0,
			) as *mut Rsp<O>)
		}
	}
}

fn lookup(name: &str) -> Option<u64> {
	let (cmd, mut rsp) = ops::Lookup::create(name);
	get_filesystem_driver()
		.unwrap()
		.lock()
		.send_command(cmd.as_ref(), rsp.as_mut());
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
		.send_command(cmd.as_ref(), rsp.as_mut());
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
					.send_command(cmd.as_ref(), rsp.as_mut());

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
				.send_command(cmd.as_ref(), rsp.as_mut());
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
		let mut len = buf.len();
		if len > MAX_WRITE_LEN {
			debug!(
				"Writing longer than max_write_len: {} > {}",
				buf.len(),
				MAX_WRITE_LEN
			);
			len = MAX_WRITE_LEN;
		}
		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, mut rsp) = ops::Write::create(nid, fh, &buf[..len], self.offset as u64);
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());

			if unsafe { rsp.out_header.assume_init_ref().error } < 0 {
				return Err(IoError::EIO);
			}

			let rsp_size = unsafe { rsp.op_header.assume_init_ref().size };
			let len: usize = if rsp_size > buf.len().try_into().unwrap() {
				buf.len()
			} else {
				rsp_size.try_into().unwrap()
			};
			self.offset += len;
			Ok(len)
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
				.send_command(cmd.as_ref(), rsp.as_mut());

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
				.send_command(cmd.as_ref(), rsp.as_mut());
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

#[derive(Debug)]
pub(crate) struct FuseDirectory;

impl FuseDirectory {
	pub const fn new() -> Self {
		FuseDirectory {}
	}
}

impl VfsNode for FuseDirectory {
	/// Returns the node type
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn traverse_readdir(&self, components: &mut Vec<&str>) -> Result<Vec<DirectoryEntry>, IoError> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

		debug!("FUSE opendir: {}", path);

		let fuse_nid = lookup(&path).ok_or(IoError::ENOENT)?;

		// Opendir
		// Flag 0x10000 for O_DIRECTORY might not be necessary
		let (mut cmd, mut rsp) = ops::Open::create(fuse_nid, 0x10000);
		cmd.in_header.opcode = fuse_abi::Opcode::Opendir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		let fuse_fh = unsafe { rsp.op_header.assume_init_ref().fh };

		debug!("FUSE readdir: {}", path);

		// Linux seems to allocate a single page to store the dirfile
		let len = MAX_READ_LEN as u32;
		let mut offset: usize = 0;

		// read content of the directory
		let (mut cmd, mut rsp) = ops::Read::create(fuse_nid, fuse_fh, len, 0);
		cmd.in_header.opcode = fuse_abi::Opcode::Readdir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());

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
			// Allign to dirent struct
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
			.send_command(cmd.as_ref(), rsp.as_mut());

		Ok(entries)
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

		debug!("FUSE stat: {}", path);

		// Is there a better way to implement this?
		let (cmd, mut rsp) = ops::Lookup::create(&path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());

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
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

		debug!("FUSE lstat: {}", path);

		let (cmd, mut rsp) = ops::Lookup::create(&path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());

		let attr = unsafe { rsp.op_header.assume_init().attr };
		Ok(FileAttr::from(attr))
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

		debug!("FUSE open: {}, {:?} {:?}", path, opt, mode);

		let file = FuseFileHandle::new();

		// 1.FUSE_INIT to create session
		// Already done
		let mut file_guard = block_on(async { Ok(file.0.lock().await) }, None)?;

		// Differentiate between opening and creating new file, since fuse does not support O_CREAT on open.
		if !opt.contains(OpenOption::O_CREAT) {
			// 2.FUSE_LOOKUP(FUSE_ROOT_ID, “foo”) -> nodeid
			file_guard.fuse_nid = lookup(&path);

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
				.send_command(cmd.as_ref(), rsp.as_mut());
			file_guard.fuse_fh = Some(unsafe { rsp.op_header.assume_init_ref().fh });
		} else {
			// Create file (opens implicitly, returns results from both lookup and open calls)
			let (cmd, mut rsp) =
				ops::Create::create(&path, opt.bits().try_into().unwrap(), mode.bits());
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());

			let inner = unsafe { rsp.op_header.assume_init() };
			file_guard.fuse_nid = Some(inner.entry.nodeid);
			file_guard.fuse_fh = Some(inner.open.fh);
		}

		drop(file_guard);

		Ok(Arc::new(file))
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> core::result::Result<(), IoError> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

		let (cmd, mut rsp) = ops::Unlink::create(&path);
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		trace!("unlink answer {:?}", rsp);

		Ok(())
	}

	fn traverse_rmdir(&self, components: &mut Vec<&str>) -> core::result::Result<(), IoError> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

		let (cmd, mut rsp) = ops::Rmdir::create(&path);
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		trace!("rmdir answer {:?}", rsp);

		Ok(())
	}

	fn traverse_mkdir(
		&self,
		components: &mut Vec<&str>,
		mode: AccessPermission,
	) -> Result<(), IoError> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};
		let (cmd, mut rsp) = ops::Mkdir::create(&path, mode.bits());

		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
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
		driver.lock().send_command(cmd.as_ref(), rsp.as_mut());
		trace!("fuse init answer: {:?}", rsp);

		let mount_point = format!("/{}", driver.lock().get_mount_point());
		info!("Mounting virtio-fs at {}", mount_point);
		fs::FILESYSTEM
			.get()
			.unwrap()
			.mount(mount_point.as_str(), Box::new(FuseDirectory::new()))
			.expect("Mount failed. Duplicate mount_point?");
	}
}
