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
use core::{fmt, future, u32, u8};

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
	fn send_command<const CODE: u32>(
		&mut self,
		cmd: &<Op<CODE> as OpTrait>::Cmd,
		rsp: &mut <Op<CODE> as OpTrait>::Rsp,
	) where
		Op<CODE>: OpTrait;

	fn get_mount_point(&self) -> String;
}

/// Marker trait, which signals that a struct is a valid Fuse command.
/// Struct has to be repr(C)!
pub(crate) unsafe trait FuseIn {}
/// Marker trait, which signals that a struct is a valid Fuse response.
/// Struct has to be repr(C)!
pub(crate) unsafe trait FuseOut {}

unsafe impl FuseIn for fuse_abi::InitIn {}
unsafe impl FuseOut for fuse_abi::InitOut {}
unsafe impl FuseIn for fuse_abi::ReadIn {}
unsafe impl FuseIn for fuse_abi::WriteIn {}
unsafe impl FuseOut for fuse_abi::WriteOut {}
unsafe impl FuseOut for fuse_abi::ReadOut {}
unsafe impl FuseIn for fuse_abi::LookupIn {}
unsafe impl FuseIn for fuse_abi::ReadlinkIn {}
unsafe impl FuseOut for fuse_abi::ReadlinkOut {}
unsafe impl FuseOut for fuse_abi::AttrOut {}
unsafe impl FuseOut for fuse_abi::EntryOut {}
unsafe impl FuseIn for fuse_abi::CreateIn {}
unsafe impl FuseOut for fuse_abi::CreateOut {}
unsafe impl FuseIn for fuse_abi::OpenIn {}
unsafe impl FuseOut for fuse_abi::OpenOut {}
unsafe impl FuseIn for fuse_abi::ReleaseIn {}
unsafe impl FuseOut for fuse_abi::ReleaseOut {}
unsafe impl FuseIn for fuse_abi::RmdirIn {}
unsafe impl FuseOut for fuse_abi::RmdirOut {}
unsafe impl FuseIn for fuse_abi::MkdirIn {}
unsafe impl FuseIn for fuse_abi::UnlinkIn {}
unsafe impl FuseOut for fuse_abi::UnlinkOut {}
unsafe impl FuseIn for fuse_abi::LseekIn {}
unsafe impl FuseOut for fuse_abi::LseekOut {}
unsafe impl FuseIn for fuse_abi::PollIn {}
unsafe impl FuseOut for fuse_abi::PollOut {}

pub(crate) trait OpTrait {
	type InStruct: FuseIn + core::fmt::Debug;
	type InPayload: ?Sized;
	type OutStruct: FuseOut + core::fmt::Debug;

	type Cmd: ?Sized + AsSliceU8 = Cmd<Self::InStruct, Self::InPayload>;
	type Rsp: ?Sized + AsSliceU8 = Rsp<Self::OutStruct>;
}

pub(crate) struct Op<const CODE: u32>;

impl OpTrait for Op<{ fuse_abi::Opcode::Init as u32 }> {
	type InStruct = fuse_abi::InitIn;
	type InPayload = ();
	type OutStruct = fuse_abi::InitOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Create as u32 }> {
	type InStruct = fuse_abi::CreateIn;
	type InPayload = CStr;
	type OutStruct = fuse_abi::CreateOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Open as u32 }> {
	type InStruct = fuse_abi::OpenIn;
	type InPayload = ();
	type OutStruct = fuse_abi::OpenOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Write as u32 }> {
	type InStruct = fuse_abi::WriteIn;
	type InPayload = [u8];
	type OutStruct = fuse_abi::WriteOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Read as u32 }> {
	type InStruct = fuse_abi::ReadIn;
	type InPayload = ();
	type OutStruct = fuse_abi::ReadOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Lseek as u32 }> {
	type InStruct = fuse_abi::LseekIn;
	type InPayload = ();
	type OutStruct = fuse_abi::LseekOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Readlink as u32 }> {
	type InStruct = fuse_abi::ReadlinkIn;
	type InPayload = ();
	type OutStruct = fuse_abi::ReadlinkOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Release as u32 }> {
	type InStruct = fuse_abi::ReleaseIn;
	type InPayload = ();
	type OutStruct = fuse_abi::ReleaseOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Mkdir as u32 }> {
	type InStruct = fuse_abi::MkdirIn;
	type InPayload = CStr;
	type OutStruct = fuse_abi::EntryOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Unlink as u32 }> {
	type InStruct = fuse_abi::UnlinkIn;
	type InPayload = CStr;
	type OutStruct = fuse_abi::UnlinkOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Rmdir as u32 }> {
	type InStruct = fuse_abi::RmdirIn;
	type InPayload = CStr;
	type OutStruct = fuse_abi::RmdirOut;
}

impl OpTrait for Op<{ fuse_abi::Opcode::Lookup as u32 }> {
	type InStruct = fuse_abi::LookupIn;
	type InPayload = CStr;
	type OutStruct = fuse_abi::EntryOut;
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
pub(crate) struct ReqPart<H, T: fmt::Debug, P: ?Sized> {
	common_header: H,
	op_header: T,
	payload: P,
}

type Cmd<T, P> = ReqPart<fuse_abi::InHeader, T, P>;
type Rsp<T> = ReqPart<fuse_abi::OutHeader, MaybeUninit<T>, [MaybeUninit<u8>]>;

impl<T: FuseIn + core::fmt::Debug, P: ?Sized> AsSliceU8 for Cmd<T, P> {
	fn len(&self) -> usize {
		self.common_header.len.try_into().unwrap()
	}
}

impl<T: FuseOut + core::fmt::Debug> AsSliceU8 for Rsp<T> {}

impl<H, T: fmt::Debug, P: ?Sized> ReqPart<H, T, P>
where
	Self: core::ptr::Pointee<Metadata = usize>,
{
	// MaybeUninit does not accept DSTs as type parameter
	unsafe fn new_uninit(len: usize) -> Box<Self> {
		unsafe {
			Box::from_raw(core::ptr::from_raw_parts_mut::<Self>(
				alloc(
					Layout::new::<ReqPart<H, T, ()>>()
						.extend(Layout::array::<u8>(len).expect("The length is too much."))
						.expect("The layout size overflowed.")
						.0 // We don't need the offset of `data_header` inside the type (the second element of the tuple)
						.pad_to_align(),
				) as *mut (),
				len,
			))
		}
	}
}

// We create the objects through the Operation struct rather than the Cmd struct to be able access the Opcode.

impl<const O: u32> Op<O>
where
	Self: OpTrait<Cmd = ReqPart<fuse_abi::InHeader, <Self as OpTrait>::InStruct, ()>>,
{
	fn new_cmd(
		nodeid: u64,
		op_header: <Self as OpTrait>::InStruct,
	) -> Box<ReqPart<fuse_abi::InHeader, <Self as OpTrait>::InStruct, ()>> {
		Box::new(ReqPart {
			common_header: fuse_abi::InHeader {
				len: Layout::new::<<Self as OpTrait>::Cmd>().size() as u32,
				opcode: O,
				nodeid,
				unique: 1,
				..Default::default()
			},
			op_header,
			payload: (),
		})
	}
}

impl<const O: u32> Op<O>
where
	Self: OpTrait,
{
	fn cmd_with_capacity(
		nodeid: u64,
		op_header: <Op<O> as OpTrait>::InStruct,
		len: usize,
	) -> Box<ReqPart<fuse_abi::InHeader, <Self as OpTrait>::InStruct, [u8]>> {
		let mut cmd = unsafe { Cmd::new_uninit(len) };
		cmd.common_header = fuse_abi::InHeader {
			len: core::mem::size_of_val(cmd.as_ref())
				.try_into()
				.expect("The command is too large"),
			opcode: O,
			nodeid,
			unique: 1,
			..Default::default()
		};
		cmd.op_header = op_header;
		cmd
	}
}

impl<const O: u32> Op<O>
where
	Self: OpTrait<Cmd = ReqPart<fuse_abi::InHeader, <Self as OpTrait>::InStruct, [u8]>>,
{
	fn cmd_from_array(
		nodeid: u64,
		op_header: <Self as OpTrait>::InStruct,
		data: &[u8],
	) -> Box<<Self as OpTrait>::Cmd> {
		let mut cmd = Self::cmd_with_capacity(nodeid, op_header, data.len());
		cmd.payload.copy_from_slice(data);
		cmd
	}
}

impl<const O: u32> Op<O>
where
	Self: OpTrait<Cmd = ReqPart<fuse_abi::InHeader, <Self as OpTrait>::InStruct, CStr>>,
{
	fn cmd_from_str(
		nodeid: u64,
		op_header: <Self as OpTrait>::InStruct,
		str: &str,
	) -> Box<<Self as OpTrait>::Cmd> {
		let str_bytes = str.as_bytes();
		// Plus one for the NUL terminator
		let mut cmd = Self::cmd_with_capacity(nodeid, op_header, str_bytes.len() + 1);
		cmd.payload[..str_bytes.len()].copy_from_slice(str_bytes);
		cmd.payload[str_bytes.len()] = b'\0';
		unsafe { core::intrinsics::transmute(cmd) }
	}
}

fn create_init() -> (
	Box<<Op<{ fuse_abi::Opcode::Init as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::InitOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Init as u32 }>::new_cmd(
		fuse_abi::ROOT_ID,
		fuse_abi::InitIn {
			major: 7,
			minor: 31,
			max_readahead: 0,
			flags: 0,
		},
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::InitOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::InitOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::InitOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_create(
	path: &str,
	flags: u32,
	mode: u32,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Create as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::CreateOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Create as u32 }>::cmd_from_str(
		fuse_abi::ROOT_ID,
		fuse_abi::CreateIn {
			flags,
			mode,
			..Default::default()
		},
		path,
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::CreateOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::CreateOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::CreateOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_open(
	nid: u64,
	flags: u32,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Open as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::OpenOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Open as u32 }>::new_cmd(
		nid,
		fuse_abi::OpenIn {
			flags,
			..Default::default()
		},
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::OpenOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::OpenOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::OpenOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

// TODO: do write zerocopy?
fn create_write(
	nid: u64,
	fh: u64,
	buf: &[u8],
	offset: u64,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Write as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::WriteOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Write as u32 }>::cmd_from_array(
		nid,
		fuse_abi::WriteIn {
			fh,
			offset,
			size: buf.len().try_into().unwrap(),
			..Default::default()
		},
		buf,
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::WriteOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::WriteOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::WriteOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_read(
	nid: u64,
	fh: u64,
	size: u32,
	offset: u64,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Read as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::ReadOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Read as u32 }>::new_cmd(
		nid,
		fuse_abi::ReadIn {
			fh,
			offset,
			size,
			..Default::default()
		},
	);

	let len = core::mem::size_of::<fuse_abi::OutHeader>()
		+ core::mem::size_of::<fuse_abi::ReadOut>()
		+ usize::try_from(size).unwrap();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::ReadOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, size.try_into().unwrap())
			as *mut Rsp<fuse_abi::ReadOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_lseek(
	nid: u64,
	fh: u64,
	offset: isize,
	whence: SeekWhence,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Lseek as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::LseekOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Lseek as u32 }>::new_cmd(
		nid,
		fuse_abi::LseekIn {
			fh,
			offset: offset.try_into().unwrap(),
			whence: num::ToPrimitive::to_u32(&whence).unwrap(),
			..Default::default()
		},
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::LseekOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::LseekOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::LseekOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_readlink(
	nid: u64,
	size: u32,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Readlink as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::ReadlinkOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Readlink as u32 }>::new_cmd(nid, fuse_abi::ReadlinkIn {});

	let len = core::mem::size_of::<fuse_abi::OutHeader>()
		+ core::mem::size_of::<fuse_abi::ReadlinkOut>()
		+ usize::try_from(size).unwrap();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::ReadlinkOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, size.try_into().unwrap())
			as *mut Rsp<fuse_abi::ReadlinkOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_release(
	nid: u64,
	fh: u64,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Release as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::ReleaseOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Release as u32 }>::new_cmd(
		nid,
		fuse_abi::ReleaseIn {
			fh,
			..Default::default()
		},
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::ReleaseOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::ReleaseOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::ReleaseOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_poll(
	nid: u64,
	fh: u64,
	kh: u64,
	event: PollEvent,
) -> (Box<Cmd<fuse_abi::PollIn>>, Box<Rsp<fuse_abi::PollOut>>) {
	let len = core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<fuse_abi::PollIn>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::PollIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_abi::PollIn>;
		(*raw).header = create_in_header::<fuse_abi::PollIn>(nid, fuse_abi::Opcode::Poll);
		(*raw).cmd = fuse_abi::PollIn {
			fh,
			kh,
			events: event.bits() as u32,
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::PollOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::PollOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::PollOut>;
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_mkdir(
	path: &str,
	mode: u32,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Mkdir as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::EntryOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Mkdir as u32 }>::cmd_from_str(
		fuse_abi::ROOT_ID,
		fuse_abi::MkdirIn {
			mode,
			..Default::default()
		},
		path,
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::EntryOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::EntryOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::EntryOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_unlink(
	name: &str,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Unlink as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::UnlinkOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Unlink as u32 }>::cmd_from_str(
		fuse_abi::ROOT_ID,
		fuse_abi::UnlinkIn {},
		name,
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::UnlinkOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::UnlinkOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::UnlinkOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_rmdir(
	name: &str,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Rmdir as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::RmdirOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Rmdir as u32 }>::cmd_from_str(
		fuse_abi::ROOT_ID,
		fuse_abi::RmdirIn {},
		name,
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::RmdirOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::RmdirOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::RmdirOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_lookup(
	name: &str,
) -> (
	Box<<Op<{ fuse_abi::Opcode::Lookup as u32 }> as OpTrait>::Cmd>,
	Box<Rsp<fuse_abi::EntryOut>>,
) {
	let cmd = Op::<{ fuse_abi::Opcode::Lookup as u32 }>::cmd_from_str(
		fuse_abi::ROOT_ID,
		fuse_abi::LookupIn {},
		name,
	);

	let len =
		core::mem::size_of::<fuse_abi::OutHeader>() + core::mem::size_of::<fuse_abi::EntryOut>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::OutHeader>(),
			core::mem::align_of::<fuse_abi::EntryOut>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_abi::EntryOut>;
		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn lookup(name: &str) -> Option<u64> {
	let (cmd, mut rsp) = create_lookup(name);
	get_filesystem_driver()
		.unwrap()
		.lock()
		.send_command::<{ fuse_abi::Opcode::Lookup as u32 }>(cmd.as_ref(), rsp.as_mut());
	if rsp.common_header.error == 0 {
		Some(unsafe { rsp.op_header.assume_init().nodeid })
	} else {
		None
	}
}

fn readlink(nid: u64) -> Result<String, IoError> {
	let len = MAX_READ_LEN as u32;
	let (cmd, mut rsp) = create_readlink(nid, len);
	get_filesystem_driver()
		.unwrap()
		.lock()
		.send_command::<{ fuse_abi::Opcode::Readlink as u32 }>(cmd.as_ref(), rsp.as_mut());
	let len: usize = if rsp.common_header.len as usize
		- ::core::mem::size_of::<fuse_abi::OutHeader>()
		- ::core::mem::size_of::<fuse_abi::ReadlinkOut>()
		>= len.try_into().unwrap()
	{
		len.try_into().unwrap()
	} else {
		rsp.common_header.len as usize
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
				let (cmd, mut rsp) = create_poll(nid, fh, kh, events);
				get_filesystem_driver()
					.ok_or(IoError::ENOSYS)?
					.lock()
					.send_command(cmd.as_ref(), rsp.as_mut());

				if rsp.header.error < 0 {
					Poll::Ready(Err(IoError::EIO))
				} else {
					let revents = unsafe {
						PollEvent::from_bits(i16::try_from(rsp.rsp.assume_init().revents).unwrap())
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
			let (cmd, mut rsp) = create_read(nid, fh, len.try_into().unwrap(), self.offset as u64);
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command::<{ fuse_abi::Opcode::Read as u32 }>(cmd.as_ref(), rsp.as_mut());
			let len: usize = if rsp.common_header.len as usize
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
				>= len
			{
				len
			} else {
				rsp.common_header.len as usize
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
			let (cmd, mut rsp) = create_write(nid, fh, &buf[..len], self.offset as u64);
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command::<{ fuse_abi::Opcode::Write as u32 }>(cmd.as_ref(), rsp.as_mut());

			if rsp.common_header.error < 0 {
				return Err(IoError::EIO);
			}

			let rsp_size = unsafe { rsp.op_header.assume_init().size };
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
			let (cmd, mut rsp) = create_lseek(nid, fh, offset, whence);
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command::<{ fuse_abi::Opcode::Lseek as u32 }>(cmd.as_ref(), rsp.as_mut());

			if rsp.common_header.error < 0 {
				return Err(IoError::EIO);
			}

			let rsp_offset = unsafe { rsp.op_header.assume_init().offset };

			Ok(rsp_offset.try_into().unwrap())
		} else {
			Err(IoError::EIO)
		}
	}
}

impl Drop for FuseFileHandleInner {
	fn drop(&mut self) {
		if self.fuse_nid.is_some() && self.fuse_fh.is_some() {
			let (cmd, mut rsp) = create_release(self.fuse_nid.unwrap(), self.fuse_fh.unwrap());
			get_filesystem_driver()
				.unwrap()
				.lock()
				.send_command::<{ fuse_abi::Opcode::Release as u32 }>(cmd.as_ref(), rsp.as_mut());
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
		let (mut cmd, mut rsp) = create_open(fuse_nid, 0x10000);
		cmd.common_header.opcode = fuse_abi::Opcode::Opendir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command::<{ fuse_abi::Opcode::Open as u32 }>(cmd.as_ref(), rsp.as_mut());
		let fuse_fh = unsafe { rsp.op_header.assume_init().fh };

		debug!("FUSE readdir: {}", path);

		// Linux seems to allocate a single page to store the dirfile
		let len = MAX_READ_LEN as u32;
		let mut offset: usize = 0;

		// read content of the directory
		let (mut cmd, mut rsp) = create_read(fuse_nid, fuse_fh, len, 0);
		cmd.common_header.opcode = fuse_abi::Opcode::Readdir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command::<{ fuse_abi::Opcode::Read as u32 }>(cmd.as_ref(), rsp.as_mut());

		let len: usize = if rsp.common_header.len as usize
			- ::core::mem::size_of::<fuse_abi::OutHeader>()
			- ::core::mem::size_of::<fuse_abi::ReadOut>()
			>= len.try_into().unwrap()
		{
			len.try_into().unwrap()
		} else {
			rsp.common_header.len as usize
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
		};

		if len <= core::mem::size_of::<fuse_abi::Dirent>() {
			debug!("FUSE no new dirs");
			return Err(IoError::ENOENT);
		}

		let mut entries: Vec<DirectoryEntry> = Vec::new();
		while rsp.common_header.len as usize - offset > core::mem::size_of::<fuse_abi::Dirent>() {
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

		let (cmd, mut rsp) = create_release(fuse_nid, fuse_fh);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command::<{ fuse_abi::Opcode::Release as u32 }>(cmd.as_ref(), rsp.as_mut());

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
		let (cmd, mut rsp) = create_lookup(&path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command::<{ fuse_abi::Opcode::Lookup as u32 }>(cmd.as_ref(), rsp.as_mut());

		if rsp.common_header.error != 0 {
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

		let (cmd, mut rsp) = create_lookup(&path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command::<{ fuse_abi::Opcode::Lookup as u32 }>(cmd.as_ref(), rsp.as_mut());

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
				create_open(file_guard.fuse_nid.unwrap(), opt.bits().try_into().unwrap());
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command::<{ fuse_abi::Opcode::Open as u32 }>(cmd.as_ref(), rsp.as_mut());
			file_guard.fuse_fh = Some(unsafe { rsp.op_header.assume_init().fh });
		} else {
			// Create file (opens implicitly, returns results from both lookup and open calls)
			let (cmd, mut rsp) = create_create(&path, opt.bits().try_into().unwrap(), mode.bits());
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command::<{ fuse_abi::Opcode::Create as u32 }>(cmd.as_ref(), rsp.as_mut());

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

		let (cmd, mut rsp) = create_unlink(&path);
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command::<{ fuse_abi::Opcode::Unlink as u32 }>(cmd.as_ref(), rsp.as_mut());
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

		let (cmd, mut rsp) = create_rmdir(&path);
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command::<{ fuse_abi::Opcode::Rmdir as u32 }>(cmd.as_ref(), rsp.as_mut());
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
		let (cmd, mut rsp) = create_mkdir(&path, mode.bits());

		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command::<{ fuse_abi::Opcode::Mkdir as u32 }>(cmd.as_ref(), rsp.as_mut());
		if rsp.common_header.error == 0 {
			Ok(())
		} else {
			Err(num::FromPrimitive::from_i32(rsp.common_header.error).unwrap())
		}
	}
}

pub(crate) fn init() {
	debug!("Try to initialize fuse filesystem");

	if let Some(driver) = get_filesystem_driver() {
		let (cmd, mut rsp) = create_init();
		driver
			.lock()
			.send_command::<{ fuse_abi::Opcode::Init as u32 }>(cmd.as_ref(), rsp.as_mut());
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
