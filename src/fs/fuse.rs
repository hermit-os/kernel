use alloc::alloc::{alloc, Layout};
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
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
	fn send_command<const CODE: u32>(&mut self, cmd: &<Op<CODE> as OpTrait>::Cmd, rsp: &mut <Op<CODE> as OpTrait>::Rsp)
	where
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
	type OutStruct: FuseOut + core::fmt::Debug;

	type Cmd: ?Sized + AsSliceU8 = Cmd<Self::InStruct>;
	type Rsp: ?Sized + AsSliceU8 = Rsp<Self::OutStruct>;
}

pub(crate) struct Op<const CODE: u32>;

impl OpTrait for Op<{fuse_abi::Opcode::Init as u32}> {
	type InStruct = fuse_abi::InitIn;
	type OutStruct = fuse_abi::InitOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Create as u32}> {
	type InStruct = fuse_abi::CreateIn;
	type OutStruct = fuse_abi::CreateOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Open as u32}> {
	type InStruct = fuse_abi::OpenIn;
	type OutStruct = fuse_abi::OpenOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Write as u32}> {
	type InStruct = fuse_abi::WriteIn;
	type OutStruct = fuse_abi::WriteOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Read as u32}> {
	type InStruct = fuse_abi::ReadIn;
	type OutStruct = fuse_abi::ReadOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Lseek as u32}> {
	type InStruct = fuse_abi::LseekIn;
	type OutStruct = fuse_abi::LseekOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Readlink as u32}> {
	type InStruct = fuse_abi::ReadlinkIn;
	type OutStruct = fuse_abi::ReadlinkOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Release as u32}> {
	type InStruct = fuse_abi::ReleaseIn;
	type OutStruct = fuse_abi::ReleaseOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Mkdir as u32}> {
	type InStruct = fuse_abi::MkdirIn;
	type OutStruct = fuse_abi::EntryOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Unlink as u32}> {
	type InStruct = fuse_abi::UnlinkIn;
	type OutStruct = fuse_abi::UnlinkOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Rmdir as u32}> {
	type InStruct = fuse_abi::RmdirIn;
	type OutStruct = fuse_abi::RmdirOut;
}

impl OpTrait for Op<{fuse_abi::Opcode::Lookup as u32}> {
	type InStruct = fuse_abi::LookupIn;
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
pub(crate) struct Cmd<T: FuseIn + fmt::Debug> {
	header: fuse_abi::InHeader,
	cmd: T,
	extra_buffer: [u8],
}

impl<T: FuseIn + core::fmt::Debug> AsSliceU8 for Cmd<T> {
	fn len(&self) -> usize {
		self.header.len.try_into().unwrap()
	}
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct Rsp<T: FuseOut + fmt::Debug> {
	header: fuse_abi::OutHeader,
	rsp: MaybeUninit<T>,
	extra_buffer: [MaybeUninit<u8>],
}

impl<T: FuseOut + core::fmt::Debug> AsSliceU8 for Rsp<T> {
	fn len(&self) -> usize {
		self.header.len.try_into().unwrap()
	}
}

fn create_in_header<T>(nodeid: u64, opcode: fuse_abi::Opcode) -> fuse_abi::InHeader
where
	T: FuseIn,
{
	fuse_abi::InHeader {
		len: (core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<T>()) as u32,
		opcode: opcode as u32,
		unique: 1,
		nodeid,
		..Default::default()
	}
}

fn create_init() -> (Box<Cmd<fuse_abi::InitIn>>, Box<Rsp<fuse_abi::InitOut>>) {
	let len = core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<fuse_abi::InitIn>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::InitIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_abi::InitIn>;
		(*raw).header =
			create_in_header::<fuse_abi::InitIn>(fuse_abi::ROOT_ID, fuse_abi::Opcode::Init);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).cmd = fuse_abi::InitIn {
			major: 7,
			minor: 31,
			max_readahead: 0,
			flags: 0,
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_create(
	path: &str,
	flags: u32,
	mode: u32,
) -> (Box<Cmd<fuse_abi::CreateIn>>, Box<Rsp<fuse_abi::CreateOut>>) {
	let slice = path.as_bytes();
	let len = core::mem::size_of::<fuse_abi::InHeader>()
		+ core::mem::size_of::<fuse_abi::CreateIn>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::CreateIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1)
			as *mut Cmd<fuse_abi::CreateIn>;
		(*raw).header =
			create_in_header::<fuse_abi::CreateIn>(fuse_abi::ROOT_ID, fuse_abi::Opcode::Create);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).cmd = fuse_abi::CreateIn {
			flags,
			mode,
			..Default::default()
		};
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_open(nid: u64, flags: u32) -> (Box<Cmd<fuse_abi::OpenIn>>, Box<Rsp<fuse_abi::OpenOut>>) {
	let len = core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<fuse_abi::OpenIn>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::OpenIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_abi::OpenIn>;
		(*raw).header = create_in_header::<fuse_abi::OpenIn>(nid, fuse_abi::Opcode::Open);
		(*raw).cmd = fuse_abi::OpenIn {
			flags,
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

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
) -> (Box<Cmd<fuse_abi::WriteIn>>, Box<Rsp<fuse_abi::WriteOut>>) {
	let len = core::mem::size_of::<fuse_abi::InHeader>()
		+ core::mem::size_of::<fuse_abi::WriteIn>()
		+ buf.len();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::WriteIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw =
			core::ptr::slice_from_raw_parts_mut(data, buf.len()) as *mut Cmd<fuse_abi::WriteIn>;
		(*raw).header = fuse_abi::InHeader {
			len: len.try_into().unwrap(),
			opcode: fuse_abi::Opcode::Write as u32,
			unique: 1,
			nodeid: nid,
			..Default::default()
		};
		(*raw).cmd = fuse_abi::WriteIn {
			fh,
			offset,
			size: buf.len().try_into().unwrap(),
			..Default::default()
		};
		(*raw).extra_buffer.copy_from_slice(buf);

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

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
) -> (Box<Cmd<fuse_abi::ReadIn>>, Box<Rsp<fuse_abi::ReadOut>>) {
	let len = core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<fuse_abi::ReadIn>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::ReadIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_abi::ReadIn>;
		(*raw).header = create_in_header::<fuse_abi::ReadIn>(nid, fuse_abi::Opcode::Read);
		(*raw).cmd = fuse_abi::ReadIn {
			fh,
			offset,
			size,
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

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
) -> (Box<Cmd<fuse_abi::LseekIn>>, Box<Rsp<fuse_abi::LseekOut>>) {
	let len =
		core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<fuse_abi::LseekIn>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::LseekIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_abi::LseekIn>;
		(*raw).header = fuse_abi::InHeader {
			len: len.try_into().unwrap(),
			opcode: fuse_abi::Opcode::Lseek as u32,
			unique: 1,
			nodeid: nid,
			..Default::default()
		};
		(*raw).cmd = fuse_abi::LseekIn {
			fh,
			offset: offset.try_into().unwrap(),
			whence: num::ToPrimitive::to_u32(&whence).unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_readlink(
	nid: u64,
	size: u32,
) -> (
	Box<Cmd<fuse_abi::ReadlinkIn>>,
	Box<Rsp<fuse_abi::ReadlinkOut>>,
) {
	let len =
		core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<fuse_abi::ReadlinkIn>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::ReadlinkIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_abi::ReadlinkIn>;
		(*raw).header = create_in_header::<fuse_abi::ReadlinkIn>(nid, fuse_abi::Opcode::Readlink);
		(*raw).header.len = len.try_into().unwrap();

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_release(
	nid: u64,
	fh: u64,
) -> (
	Box<Cmd<fuse_abi::ReleaseIn>>,
	Box<Rsp<fuse_abi::ReleaseOut>>,
) {
	let len =
		core::mem::size_of::<fuse_abi::InHeader>() + core::mem::size_of::<fuse_abi::ReleaseIn>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::ReleaseIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_abi::ReleaseIn>;
		(*raw).header = create_in_header::<fuse_abi::ReleaseIn>(nid, fuse_abi::Opcode::Release);
		(*raw).cmd = fuse_abi::ReleaseIn {
			fh,
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

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
) -> (Box<Cmd<fuse_abi::MkdirIn>>, Box<Rsp<fuse_abi::EntryOut>>) {
	let slice = path.as_bytes();
	let len = core::mem::size_of::<fuse_abi::InHeader>()
		+ core::mem::size_of::<fuse_abi::MkdirIn>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::MkdirIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1)
			as *mut Cmd<fuse_abi::MkdirIn>;
		(*raw).header =
			create_in_header::<fuse_abi::MkdirIn>(fuse_abi::ROOT_ID, fuse_abi::Opcode::Mkdir);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).cmd = fuse_abi::MkdirIn {
			mode,
			..Default::default()
		};
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_unlink(name: &str) -> (Box<Cmd<fuse_abi::UnlinkIn>>, Box<Rsp<fuse_abi::UnlinkOut>>) {
	let slice = name.as_bytes();
	let len = core::mem::size_of::<fuse_abi::InHeader>()
		+ core::mem::size_of::<fuse_abi::UnlinkIn>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::UnlinkIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1)
			as *mut Cmd<fuse_abi::UnlinkIn>;
		(*raw).header =
			create_in_header::<fuse_abi::UnlinkIn>(fuse_abi::ROOT_ID, fuse_abi::Opcode::Unlink);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_rmdir(name: &str) -> (Box<Cmd<fuse_abi::RmdirIn>>, Box<Rsp<fuse_abi::RmdirOut>>) {
	let slice = name.as_bytes();
	let len = core::mem::size_of::<fuse_abi::InHeader>()
		+ core::mem::size_of::<fuse_abi::RmdirIn>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::RmdirIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1)
			as *mut Cmd<fuse_abi::RmdirIn>;
		(*raw).header =
			create_in_header::<fuse_abi::RmdirIn>(fuse_abi::ROOT_ID, fuse_abi::Opcode::Rmdir);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_lookup(name: &str) -> (Box<Cmd<fuse_abi::LookupIn>>, Box<Rsp<fuse_abi::EntryOut>>) {
	let slice = name.as_bytes();
	let len = core::mem::size_of::<fuse_abi::InHeader>()
		+ core::mem::size_of::<fuse_abi::LookupIn>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_abi::InHeader>(),
			core::mem::align_of::<fuse_abi::LookupIn>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1)
			as *mut Cmd<fuse_abi::LookupIn>;
		(*raw).header =
			create_in_header::<fuse_abi::LookupIn>(fuse_abi::ROOT_ID, fuse_abi::Opcode::Lookup);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

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
		(*raw).header = fuse_abi::OutHeader {
			len: len.try_into().unwrap(),
			..Default::default()
		};

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
		.send_command::<{fuse_abi::Opcode::Lookup as u32}>(cmd.as_ref(), rsp.as_mut());
	if rsp.header.error == 0 {
		Some(unsafe { rsp.rsp.assume_init().nodeid })
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
		.send_command::<{fuse_abi::Opcode::Readlink as u32}>(cmd.as_ref(), rsp.as_mut());
	let len: usize = if rsp.header.len as usize
		- ::core::mem::size_of::<fuse_abi::OutHeader>()
		- ::core::mem::size_of::<fuse_abi::ReadlinkOut>()
		>= len.try_into().unwrap()
	{
		len.try_into().unwrap()
	} else {
		rsp.header.len as usize
			- ::core::mem::size_of::<fuse_abi::OutHeader>()
			- ::core::mem::size_of::<fuse_abi::ReadlinkOut>()
	};

	Ok(String::from_utf8(unsafe {
		MaybeUninit::slice_assume_init_ref(&rsp.extra_buffer[..len]).to_vec()
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
				.send_command::<{fuse_abi::Opcode::Read as u32}>(cmd.as_ref(), rsp.as_mut());
			let len: usize = if rsp.header.len as usize
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
				>= len
			{
				len
			} else {
				rsp.header.len as usize
					- ::core::mem::size_of::<fuse_abi::OutHeader>()
					- ::core::mem::size_of::<fuse_abi::ReadOut>()
			};
			self.offset += len;

			buf[..len].copy_from_slice(unsafe {
				MaybeUninit::slice_assume_init_ref(&rsp.extra_buffer[..len])
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
				.send_command::<{fuse_abi::Opcode::Write as u32}>(cmd.as_ref(), rsp.as_mut());

			if rsp.header.error < 0 {
				return Err(IoError::EIO);
			}

			let rsp_size = unsafe { rsp.rsp.assume_init().size };
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
				.send_command::<{fuse_abi::Opcode::Lseek as u32}>(cmd.as_ref(), rsp.as_mut());

			if rsp.header.error < 0 {
				return Err(IoError::EIO);
			}

			let rsp_offset = unsafe { rsp.rsp.assume_init().offset };

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
				.send_command::<{fuse_abi::Opcode::Release as u32}>(cmd.as_ref(), rsp.as_mut());
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
		cmd.header.opcode = fuse_abi::Opcode::Opendir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command::<{fuse_abi::Opcode::Open as u32}>(cmd.as_ref(), rsp.as_mut());
		let fuse_fh = unsafe { rsp.rsp.assume_init().fh };

		debug!("FUSE readdir: {}", path);

		// Linux seems to allocate a single page to store the dirfile
		let len = MAX_READ_LEN as u32;
		let mut offset: usize = 0;

		// read content of the directory
		let (mut cmd, mut rsp) = create_read(fuse_nid, fuse_fh, len, 0);
		cmd.header.opcode = fuse_abi::Opcode::Readdir as u32;
		get_filesystem_driver()
			.ok_or(IoError::ENOSYS)?
			.lock()
			.send_command::<{fuse_abi::Opcode::Read as u32}>(cmd.as_ref(), rsp.as_mut());

		let len: usize = if rsp.header.len as usize
			- ::core::mem::size_of::<fuse_abi::OutHeader>()
			- ::core::mem::size_of::<fuse_abi::ReadOut>()
			>= len.try_into().unwrap()
		{
			len.try_into().unwrap()
		} else {
			rsp.header.len as usize
				- ::core::mem::size_of::<fuse_abi::OutHeader>()
				- ::core::mem::size_of::<fuse_abi::ReadOut>()
		};

		if len <= core::mem::size_of::<fuse_abi::Dirent>() {
			debug!("FUSE no new dirs");
			return Err(IoError::ENOENT);
		}

		let mut entries: Vec<DirectoryEntry> = Vec::new();
		while rsp.header.len as usize - offset > core::mem::size_of::<fuse_abi::Dirent>() {
			let dirent = unsafe {
				&*(rsp.extra_buffer.as_ptr().byte_add(offset) as *const fuse_abi::Dirent)
			};

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
			.send_command::<{fuse_abi::Opcode::Release as u32}>(cmd.as_ref(), rsp.as_mut());

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
			.send_command::<{fuse_abi::Opcode::Lookup as u32}>(cmd.as_ref(), rsp.as_mut());

		if rsp.header.error != 0 {
			// TODO: Correct error handling
			return Err(IoError::EIO);
		}

		let rsp = unsafe { rsp.rsp.assume_init() };
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
			.send_command::<{fuse_abi::Opcode::Lookup as u32}>(cmd.as_ref(), rsp.as_mut());

		let attr = unsafe { rsp.rsp.assume_init().attr };
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
				.send_command::<{fuse_abi::Opcode::Open as u32}>(cmd.as_ref(), rsp.as_mut());
			file_guard.fuse_fh = Some(unsafe { rsp.rsp.assume_init().fh });
		} else {
			// Create file (opens implicitly, returns results from both lookup and open calls)
			let (cmd, mut rsp) = create_create(&path, opt.bits().try_into().unwrap(), mode.bits());
			get_filesystem_driver()
				.ok_or(IoError::ENOSYS)?
				.lock()
				.send_command::<{fuse_abi::Opcode::Create as u32}>(cmd.as_ref(), rsp.as_mut());

			let inner = unsafe { rsp.rsp.assume_init() };
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
			.send_command::<{fuse_abi::Opcode::Unlink as u32}>(cmd.as_ref(), rsp.as_mut());
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
			.send_command::<{fuse_abi::Opcode::Rmdir as u32}>(cmd.as_ref(), rsp.as_mut());
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
			.send_command::<{fuse_abi::Opcode::Mkdir as u32}>(cmd.as_ref(), rsp.as_mut());
		if rsp.header.error == 0 {
			Ok(())
		} else {
			Err(num::FromPrimitive::from_i32(rsp.header.error).unwrap())
		}
	}
}

pub(crate) fn init() {
	debug!("Try to initialize fuse filesystem");

	if let Some(driver) = get_filesystem_driver() {
		let (cmd, mut rsp) = create_init();
		driver.lock().send_command::<{fuse_abi::Opcode::Init as u32}>(cmd.as_ref(), rsp.as_mut());
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
