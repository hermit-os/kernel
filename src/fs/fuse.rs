use alloc::alloc::{alloc, Layout};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::{fmt, u32, u8};

#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio::get_filesystem_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_filesystem_driver;
use crate::drivers::virtio::virtqueue::AsSliceU8;
use crate::syscalls::fs::{
	self, Dirent, FileAttr, FileError, FilePerms, PosixFile, PosixFileSystem, SeekWhence,
};

// response out layout eg @ https://github.com/zargony/fuse-rs/blob/bf6d1cf03f3277e35b580f3c7b9999255d72ecf3/src/ll/request.rs#L44
// op in/out sizes/layout: https://github.com/hanwen/go-fuse/blob/204b45dba899dfa147235c255908236d5fde2d32/fuse/opcode.go#L439
// possible responses for command: qemu/tools/virtiofsd/fuse_lowlevel.h

const FUSE_ROOT_ID: u64 = 1;
const MAX_READ_LEN: usize = 1024 * 64;
const MAX_WRITE_LEN: usize = 1024 * 64;

const U64_SIZE: u32 = ::core::mem::size_of::<u64>() as u32;

const S_IFLNK: u32 = 40960;
const S_IFMT: u32 = 61440;

const FUSE_GETATTR_FH: u32 = 1 << 0;

#[repr(C)]
#[derive(Debug)]
pub struct fuse_dirent {
	pub d_ino: u64,
	pub d_off: u64,
	pub d_namelen: u32,
	pub d_type: u32,
	pub d_name: [u8; 0],
}

pub trait FuseInterface {
	fn send_command<S, T>(&mut self, cmd: &Cmd<S>, rsp: &mut Rsp<T>)
	where
		S: FuseIn + core::fmt::Debug,
		T: FuseOut + core::fmt::Debug;

	fn get_mount_point(&self) -> String;
}

pub struct Fuse;

impl PosixFileSystem for Fuse {
	fn open(&self, path: &str, perms: FilePerms) -> Result<Box<dyn PosixFile + Send>, FileError> {
		let mut file = FuseFile {
			fuse_nid: None,
			fuse_fh: None,
			offset: 0,
		};
		// 1.FUSE_INIT to create session
		// Already done

		// Differentiate between opening and creating new file, since fuse does not support O_CREAT on open.
		if !perms.creat {
			// 2.FUSE_LOOKUP(FUSE_ROOT_ID, “foo”) -> nodeid
			file.fuse_nid = self.lookup(path);

			if file.fuse_nid.is_none() {
				warn!("Fuse lookup seems to have failed!");
				return Err(FileError::ENOENT);
			}

			// 3.FUSE_OPEN(nodeid, O_RDONLY) -> fh
			let (cmd, mut rsp) = create_open(file.fuse_nid.unwrap(), perms.raw);
			get_filesystem_driver()
				.ok_or(FileError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());
			file.fuse_fh = Some(unsafe { rsp.rsp.assume_init().fh });
		} else {
			// Create file (opens implicitly, returns results from both lookup and open calls)
			let (cmd, mut rsp) = create_create(path, perms.raw, perms.mode);
			get_filesystem_driver()
				.ok_or(FileError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());

			let inner = unsafe { rsp.rsp.assume_init() };
			file.fuse_nid = Some(inner.entry.nodeid);
			file.fuse_fh = Some(inner.open.fh);
		}

		Ok(Box::new(file))
	}

	fn opendir(&self, path: &str) -> Result<Box<dyn PosixFile + Send>, FileError> {
		debug!("FUSE opendir: {}", path);

		let mut readdir = FuseDir {
			fuse_nid: None,
			fuse_fh: None,
			offset: 0,
			response: None,
			buffer_offset: 0,
		};

		// Lookup nodeid

		readdir.fuse_nid = self.lookup(path);

		if readdir.fuse_nid.is_none() {
			warn!("Fuse lookup seems to have failed!");
			return Err(FileError::ENOENT);
		}

		// Opendir
		// Flag 0x10000 for O_DIRECTORY might not be necessary
		let (mut cmd, mut rsp) = create_open(readdir.fuse_nid.unwrap(), 0x10000);
		cmd.header.opcode = Opcode::FUSE_OPENDIR as u32;
		get_filesystem_driver()
			.ok_or(FileError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		readdir.fuse_fh = Some(unsafe { rsp.rsp.assume_init().fh });

		Ok(Box::new(readdir))
	}

	fn unlink(&self, path: &str) -> core::result::Result<(), FileError> {
		let (cmd, mut rsp) = create_unlink(path);
		get_filesystem_driver()
			.ok_or(FileError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		trace!("unlink answer {:?}", rsp);

		Ok(())
	}

	fn rmdir(&self, path: &str) -> core::result::Result<(), FileError> {
		let (cmd, mut rsp) = create_rmdir(path);
		get_filesystem_driver()
			.ok_or(FileError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		trace!("rmdir answer {:?}", rsp);

		Ok(())
	}

	fn mkdir(&self, name: &str, mode: u32) -> Result<i32, FileError> {
		let (mut cmd, mut rsp) = create_mkdir(name, mode);

		get_filesystem_driver()
			.ok_or(FileError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		Ok(unsafe { rsp.rsp.assume_init().nodeid.try_into().unwrap() })
	}

	fn stat(&self, path: &str, stat: *mut FileAttr) -> Result<(), FileError> {
		debug!("FUSE stat: {}", path);

		// Is there a better way to implement this?
		let (cmd, mut rsp) = create_lookup(path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());

		if rsp.header.error != 0 {
			// TODO: Correct error handling
			return Err(FileError::EIO);
		}

		let rsp = unsafe { rsp.rsp.assume_init() };
		let attr = rsp.attr;

		if attr.mode & S_IFMT != S_IFLNK {
			unsafe {
				attr.fill_stat(stat);
			}
			Ok(())
		} else {
			self.stat(&self.readlink(rsp.nodeid)?, stat)
		}
	}

	fn lstat(&self, path: &str, stat: *mut FileAttr) -> Result<(), FileError> {
		let (cmd, mut rsp) = create_lookup(path);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());

		let attr = unsafe { rsp.rsp.assume_init().attr };

		unsafe {
			attr.fill_stat(stat);
		}

		Ok(())
	}
}

impl Fuse {
	pub fn new() -> Self {
		Self {}
	}

	pub fn send_init(&self) {
		let (cmd, mut rsp) = create_init();
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		trace!("fuse init answer: {:?}", rsp);
	}

	pub fn lookup(&self, name: &str) -> Option<u64> {
		let (cmd, mut rsp) = create_lookup(name);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		if rsp.header.error == 0 {
			Some(unsafe { rsp.rsp.assume_init().nodeid })
		} else {
			None
		}
	}

	pub fn readlink(&self, nid: u64) -> Result<String, FileError> {
		let mut len = MAX_READ_LEN as u32;
		let (cmd, mut rsp) = create_readlink(nid, len);
		get_filesystem_driver()
			.unwrap()
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());
		let len: usize = if rsp.header.len as usize
			- ::core::mem::size_of::<fuse_out_header>()
			- ::core::mem::size_of::<fuse_readlink_out>()
			>= len.try_into().unwrap()
		{
			len.try_into().unwrap()
		} else {
			rsp.header.len as usize
				- ::core::mem::size_of::<fuse_out_header>()
				- ::core::mem::size_of::<fuse_readlink_out>()
		};

		Ok(String::from_utf8(unsafe {
			MaybeUninit::slice_assume_init_ref(&rsp.extra_buffer[..len]).to_vec()
		})
		.unwrap())
	}
}

impl Default for Fuse {
	fn default() -> Self {
		Self::new()
	}
}

struct FuseFile {
	fuse_nid: Option<u64>,
	fuse_fh: Option<u64>,
	offset: usize,
}

struct FuseDir {
	fuse_nid: Option<u64>,
	fuse_fh: Option<u64>,
	offset: u64,
	response: Option<Box<Rsp<fuse_read_out>>>,
	buffer_offset: u32,
}

impl PosixFile for FuseDir {
	fn close(&mut self) -> Result<(), FileError> {
		let (mut cmd, mut rsp) = create_release(self.fuse_nid.unwrap(), self.fuse_fh.unwrap());
		cmd.header.opcode = Opcode::FUSE_RELEASEDIR as u32;
		get_filesystem_driver()
			.ok_or(FileError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());

		Ok(())
	}
	fn read(&mut self, _len: u32) -> Result<Vec<u8>, FileError> {
		// Use readdir instead
		Err(FileError::EISDIR)
	}
	fn write(&mut self, _buf: &[u8]) -> Result<u64, FileError> {
		// Not opened for writing
		Err(FileError::EBADF)
	}
	fn lseek(&mut self, _offset: isize, _whence: SeekWhence) -> Result<usize, FileError> {
		Err(FileError::ENOSYS)
	}

	fn readdir(&mut self) -> Result<*const Dirent, FileError> {
		// Check if we have to read the directory via FUSE or still have a direntry in the last respnse
		let resp: &_ = match &mut self.response {
			Some(resp)
				if resp.header.len - self.buffer_offset
					> core::mem::size_of::<fuse_dirent>().try_into().unwrap() =>
			{
				resp
			}
			option => {
				debug!("FUSE read from dirfile");
				// Linux seems to allocate a single page to store the dirfile
				let len = MAX_READ_LEN as u32;

				let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) else {
					warn!("FUSE dir not open, cannot read!");
					return Err(FileError::EBADF);
				};

				let (mut cmd, mut rsp) = create_read(nid, fh, len, self.offset as u64);
				cmd.header.opcode = Opcode::FUSE_READDIR as u32;
				if let Some(fs_driver) = get_filesystem_driver() {
					fs_driver.lock().send_command(cmd.as_ref(), rsp.as_mut());
				} else {
					return Err(FileError::ENOSYS);
				}

				let len: usize = if rsp.header.len as usize
					- ::core::mem::size_of::<fuse_out_header>()
					- ::core::mem::size_of::<fuse_read_out>()
					>= len.try_into().unwrap()
				{
					len.try_into().unwrap()
				} else {
					rsp.header.len as usize
						- ::core::mem::size_of::<fuse_out_header>()
						- ::core::mem::size_of::<fuse_read_out>()
				};

				if len <= core::mem::size_of::<fuse_dirent>() {
					debug!("FUSE no new dirs");
					return Ok(core::ptr::null());
				}

				// Keep smart pointer to response
				let rsp = option.insert(rsp);
				self.buffer_offset = 0;

				debug!("FUSE new buffer len: {}", len);
				rsp
			}
		};

		let return_ptr: *const u8 = unsafe {
			resp.extra_buffer
				.as_ptr()
				.byte_add(self.buffer_offset.try_into().unwrap()) as _
		};

		let dirent = unsafe { &*(return_ptr as *const fuse_dirent) };

		self.offset = dirent.d_off;

		self.buffer_offset += core::mem::size_of::<fuse_dirent>() as u32 + dirent.d_namelen;

		// Allign to dirent struct
		self.buffer_offset = ((self.buffer_offset) + U64_SIZE - 1) & (!(U64_SIZE - 1));

		// Check alignment
		assert!(return_ptr.is_aligned_to(U64_SIZE.try_into().unwrap()));
		Ok(return_ptr.cast())
	}

	fn fstat(&self, _stat: *mut FileAttr) -> Result<(), FileError> {
		Err(FileError::ENOSYS)
	}
}

impl PosixFile for FuseFile {
	fn close(&mut self) -> Result<(), FileError> {
		let (cmd, mut rsp) = create_release(self.fuse_nid.unwrap(), self.fuse_fh.unwrap());
		get_filesystem_driver()
			.ok_or(FileError::ENOSYS)?
			.lock()
			.send_command(cmd.as_ref(), rsp.as_mut());

		Ok(())
	}

	fn read(&mut self, len: u32) -> Result<Vec<u8>, FileError> {
		let mut len = len;
		if len as usize > MAX_READ_LEN {
			debug!("Reading longer than max_read_len: {}", len);
			len = MAX_READ_LEN as u32;
		}
		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, mut rsp) = create_read(nid, fh, len, self.offset as u64);
			get_filesystem_driver()
				.ok_or(FileError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());
			let len: usize = if rsp.header.len as usize
				- ::core::mem::size_of::<fuse_out_header>()
				- ::core::mem::size_of::<fuse_read_out>()
				>= len.try_into().unwrap()
			{
				len.try_into().unwrap()
			} else {
				rsp.header.len as usize
					- ::core::mem::size_of::<fuse_out_header>()
					- ::core::mem::size_of::<fuse_read_out>()
			};
			self.offset += len;

			Ok(unsafe { MaybeUninit::slice_assume_init_ref(&rsp.extra_buffer[..len]).to_vec() })
		} else {
			warn!("File not open, cannot read!");
			Err(FileError::ENOENT)
		}
	}

	fn write(&mut self, buf: &[u8]) -> Result<u64, FileError> {
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
				.ok_or(FileError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());

			if rsp.header.error < 0 {
				return Err(FileError::EIO);
			}

			let rsp_size = unsafe { rsp.rsp.assume_init().size };
			let len: usize = if rsp_size > buf.len().try_into().unwrap() {
				buf.len()
			} else {
				rsp_size.try_into().unwrap()
			};
			self.offset += len;
			Ok(len.try_into().unwrap())
		} else {
			warn!("File not open, cannot read!");
			Err(FileError::ENOENT)
		}
	}

	fn lseek(&mut self, offset: isize, whence: SeekWhence) -> Result<usize, FileError> {
		debug!("FUSE lseek");

		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, mut rsp) = create_lseek(nid, fh, offset, whence);
			get_filesystem_driver()
				.ok_or(FileError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());

			if rsp.header.error < 0 {
				return Err(FileError::EIO);
			}

			let rsp_offset = unsafe { rsp.rsp.assume_init().offset };

			Ok(rsp_offset.try_into().unwrap())
		} else {
			Err(FileError::EIO)
		}
	}

	fn readdir(&mut self) -> Result<*const Dirent, FileError> {
		Err(FileError::EBADF)
	}

	fn fstat(&self, stat: *mut FileAttr) -> Result<(), FileError> {
		if let (Some(nid), Some(fh)) = (self.fuse_nid, self.fuse_fh) {
			let (cmd, mut rsp) = create_getattr(nid, fh, FUSE_GETATTR_FH);
			get_filesystem_driver()
				.ok_or(FileError::ENOSYS)?
				.lock()
				.send_command(cmd.as_ref(), rsp.as_mut());

			if rsp.header.error < 0 {
				return Err(FileError::EIO);
			}

			let attr = unsafe { rsp.rsp.assume_init().attr };
			unsafe { attr.fill_stat(stat) };
			Ok(())
		} else {
			Err(FileError::EIO)
		}
	}
}

#[repr(u32)]
#[derive(Copy, Clone, Debug)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub enum Opcode {
	FUSE_LOOKUP = 1,
	FUSE_FORGET = 2, // no reply
	FUSE_GETATTR = 3,
	FUSE_SETATTR = 4,
	FUSE_READLINK = 5,
	FUSE_SYMLINK = 6,
	FUSE_MKNOD = 8,
	FUSE_MKDIR = 9,
	FUSE_UNLINK = 10,
	FUSE_RMDIR = 11,
	FUSE_RENAME = 12,
	FUSE_LINK = 13,
	FUSE_OPEN = 14,
	FUSE_READ = 15,
	FUSE_WRITE = 16,
	FUSE_STATFS = 17,
	FUSE_RELEASE = 18,
	FUSE_FSYNC = 20,
	FUSE_SETXATTR = 21,
	FUSE_GETXATTR = 22,
	FUSE_LISTXATTR = 23,
	FUSE_REMOVEXATTR = 24,
	FUSE_FLUSH = 25,
	FUSE_INIT = 26,
	FUSE_OPENDIR = 27,
	FUSE_READDIR = 28,
	FUSE_RELEASEDIR = 29,
	FUSE_FSYNCDIR = 30,
	FUSE_GETLK = 31,
	FUSE_SETLK = 32,
	FUSE_SETLKW = 33,
	FUSE_ACCESS = 34,
	FUSE_CREATE = 35,
	FUSE_INTERRUPT = 36,
	FUSE_BMAP = 37,
	FUSE_DESTROY = 38,
	FUSE_IOCTL = 39,
	FUSE_POLL = 40,
	FUSE_NOTIFY_REPLY = 41,
	FUSE_BATCH_FORGET = 42,
	FUSE_FALLOCATE = 43,
	FUSE_READDIRPLUS = 44,
	FUSE_RENAME2 = 45,
	FUSE_LSEEK = 46,

	FUSE_SETVOLNAME = 61,
	FUSE_GETXTIMES = 62,
	FUSE_EXCHANGE = 63,

	CUSE_INIT = 4096,
}

/// Marker trait, which signals that a struct is a valid Fuse command.
/// Struct has to be repr(C)!
pub unsafe trait FuseIn {}
/// Marker trait, which signals that a struct is a valid Fuse response.
/// Struct has to be repr(C)!
pub unsafe trait FuseOut {}

#[repr(C)]
#[derive(Debug)]
pub struct Cmd<T: FuseIn + fmt::Debug> {
	header: fuse_in_header,
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
pub struct Rsp<T: FuseOut + fmt::Debug> {
	header: fuse_out_header,
	rsp: MaybeUninit<T>,
	extra_buffer: [MaybeUninit<u8>],
}

impl<T: FuseOut + core::fmt::Debug> AsSliceU8 for Rsp<T> {
	fn len(&self) -> usize {
		self.header.len.try_into().unwrap()
	}
}

fn create_in_header<T>(nodeid: u64, opcode: Opcode) -> fuse_in_header
where
	T: FuseIn,
{
	fuse_in_header {
		len: (core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<T>()) as u32,
		opcode: opcode as u32,
		unique: 1,
		nodeid,
		..Default::default()
	}
}

fn create_init() -> (Box<Cmd<fuse_init_in>>, Box<Rsp<fuse_init_out>>) {
	let len = core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_init_in>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_init_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_init_in>;
		(*raw).header = create_in_header::<fuse_init_in>(FUSE_ROOT_ID, Opcode::FUSE_INIT);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).cmd = fuse_init_in {
			major: 7,
			minor: 31,
			max_readahead: 0,
			flags: 0,
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_init_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_init_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_init_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_getattr(
	nid: u64,
	fh: u64,
	flags: u32,
) -> (Box<Cmd<fuse_getattr_in>>, Box<Rsp<fuse_attr_out>>) {
	let len = core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_getattr_in>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_getattr_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_getattr_in>;
		(*raw).header = create_in_header::<fuse_getattr_in>(nid, Opcode::FUSE_GETATTR);
		(*raw).cmd = fuse_getattr_in {
			getattr_flags: flags,
			dummy: 0,
			fh: fh,
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_attr_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_attr_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_attr_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_lookup(name: &str) -> (Box<Cmd<fuse_lookup_in>>, Box<Rsp<fuse_entry_out>>) {
	let slice = name.as_bytes();
	let len = core::mem::size_of::<fuse_in_header>()
		+ core::mem::size_of::<fuse_lookup_in>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_lookup_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw =
			core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1) as *mut Cmd<fuse_lookup_in>;
		(*raw).header = create_in_header::<fuse_lookup_in>(FUSE_ROOT_ID, Opcode::FUSE_LOOKUP);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_entry_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_entry_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_entry_out>;
		(*raw).header = fuse_out_header {
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
) -> (Box<Cmd<fuse_readlink_in>>, Box<Rsp<fuse_readlink_out>>) {
	let len = core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_readlink_in>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_readlink_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_readlink_in>;
		(*raw).header = create_in_header::<fuse_readlink_in>(nid, Opcode::FUSE_READLINK);
		(*raw).header.len = len.try_into().unwrap();

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>()
		+ core::mem::size_of::<fuse_readlink_out>()
		+ usize::try_from(size).unwrap();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_readlink_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, size.try_into().unwrap())
			as *mut Rsp<fuse_readlink_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct fuse_in_header {
	pub len: u32,
	pub opcode: u32,
	pub unique: u64,
	pub nodeid: u64,
	pub uid: u32,
	pub gid: u32,
	pub pid: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_out_header {
	pub len: u32,
	pub error: i32,
	pub unique: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct fuse_init_in {
	pub major: u32,
	pub minor: u32,
	pub max_readahead: u32,
	pub flags: u32,
}

unsafe impl FuseIn for fuse_init_in {}

#[repr(C)]
#[derive(Debug, Default)]
pub struct fuse_init_out {
	pub major: u32,
	pub minor: u32,
	pub max_readahead: u32,
	pub flags: u32,
	pub max_background: u16,
	pub congestion_threshold: u16,
	pub max_write: u32,
	pub time_gran: u32,
	pub unused: [u32; 9],
}
unsafe impl FuseOut for fuse_init_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_read_in {
	pub fh: u64,
	pub offset: u64,
	pub size: u32,
	pub read_flags: u32,
	pub lock_owner: u64,
	pub flags: u32,
	pub padding: u32,
}

unsafe impl FuseIn for fuse_read_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_read_out {}
unsafe impl FuseOut for fuse_read_out {}

fn create_read(
	nid: u64,
	fh: u64,
	size: u32,
	offset: u64,
) -> (Box<Cmd<fuse_read_in>>, Box<Rsp<fuse_read_out>>) {
	let len = core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_read_in>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_read_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_read_in>;
		(*raw).header = create_in_header::<fuse_read_in>(nid, Opcode::FUSE_READ);
		(*raw).cmd = fuse_read_in {
			fh,
			offset,
			size,
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>()
		+ core::mem::size_of::<fuse_read_out>()
		+ usize::try_from(size).unwrap();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_read_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, size.try_into().unwrap())
			as *mut Rsp<fuse_read_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_lseek_in {
	pub fh: u64,
	pub offset: u64,
	pub whence: u32,
	pub padding: u32,
}
unsafe impl FuseIn for fuse_lseek_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_lseek_out {
	offset: u64,
}
unsafe impl FuseOut for fuse_lseek_out {}

fn create_lseek(
	nid: u64,
	fh: u64,
	offset: isize,
	whence: SeekWhence,
) -> (Box<Cmd<fuse_lseek_in>>, Box<Rsp<fuse_lseek_out>>) {
	let len = core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_lseek_in>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_lseek_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_lseek_in>;
		(*raw).header = fuse_in_header {
			len: len.try_into().unwrap(),
			opcode: Opcode::FUSE_LSEEK as u32,
			unique: 1,
			nodeid: nid,
			..Default::default()
		};
		(*raw).cmd = fuse_lseek_in {
			fh,
			offset: offset.try_into().unwrap(),
			whence: num::ToPrimitive::to_u32(&whence).unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_lseek_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_lseek_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_lseek_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_write_in {
	pub fh: u64,
	pub offset: u64,
	pub size: u32,
	pub write_flags: u32,
	pub lock_owner: u64,
	pub flags: u32,
	pub padding: u32,
}
unsafe impl FuseIn for fuse_write_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_write_out {
	pub size: u32,
	pub padding: u32,
}
unsafe impl FuseOut for fuse_write_out {}

// TODO: do write zerocopy?
fn create_write(
	nid: u64,
	fh: u64,
	buf: &[u8],
	offset: u64,
) -> (Box<Cmd<fuse_write_in>>, Box<Rsp<fuse_write_out>>) {
	let len =
		core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_write_in>() + buf.len();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_write_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, buf.len()) as *mut Cmd<fuse_write_in>;
		(*raw).header = fuse_in_header {
			len: len.try_into().unwrap(),
			opcode: Opcode::FUSE_WRITE as u32,
			unique: 1,
			nodeid: nid,
			..Default::default()
		};
		(*raw).cmd = fuse_write_in {
			fh,
			offset,
			size: buf.len().try_into().unwrap(),
			..Default::default()
		};
		(*raw).extra_buffer.copy_from_slice(buf);

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_write_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_write_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_write_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_open_in {
	pub flags: u32,
	pub unused: u32,
}

unsafe impl FuseIn for fuse_open_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_open_out {
	pub fh: u64,
	pub open_flags: u32,
	pub padding: u32,
}

unsafe impl FuseOut for fuse_open_out {}

fn create_open(nid: u64, flags: u32) -> (Box<Cmd<fuse_open_in>>, Box<Rsp<fuse_open_out>>) {
	let len = core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_open_in>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_open_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_open_in>;
		(*raw).header = create_in_header::<fuse_open_in>(nid, Opcode::FUSE_OPEN);
		(*raw).cmd = fuse_open_in {
			flags,
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_open_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_open_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_open_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_release_in {
	pub fh: u64,
	pub flags: u32,
	pub release_flags: u32,
	pub lock_owner: u64,
}

unsafe impl FuseIn for fuse_release_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_release_out {}
unsafe impl FuseOut for fuse_release_out {}

fn create_release(nid: u64, fh: u64) -> (Box<Cmd<fuse_release_in>>, Box<Rsp<fuse_release_out>>) {
	let len = core::mem::size_of::<fuse_in_header>() + core::mem::size_of::<fuse_release_in>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_release_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Cmd<fuse_release_in>;
		(*raw).header = create_in_header::<fuse_release_in>(nid, Opcode::FUSE_RELEASE);
		(*raw).cmd = fuse_release_in {
			fh,
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_release_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_release_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_release_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_lookup_in {}
unsafe impl FuseIn for fuse_lookup_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_readlink_in {}

unsafe impl FuseIn for fuse_readlink_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_readlink_out {}
unsafe impl FuseOut for fuse_readlink_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_getattr_in {
	pub getattr_flags: u32,
	pub dummy: u32,
	pub fh: u64,
}

unsafe impl FuseIn for fuse_getattr_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_attr_out {
	pub attr_valid: u64,
	pub attr_valid_nsec: u32,
	pub dummy: u32,
	pub attr: fuse_attr,
}

unsafe impl FuseOut for fuse_attr_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_entry_out {
	pub nodeid: u64,
	pub generation: u64,
	pub entry_valid: u64,
	pub attr_valid: u64,
	pub entry_valid_nsec: u32,
	pub attr_valid_nsec: u32,
	pub attr: fuse_attr,
}

unsafe impl FuseOut for fuse_entry_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_attr {
	pub ino: u64,
	pub size: u64,
	pub blocks: u64,
	pub atime: u64,
	pub mtime: u64,
	pub ctime: u64,
	pub atimensec: u32,
	pub mtimensec: u32,
	pub ctimensec: u32,
	pub mode: u32,
	pub nlink: u32,
	pub uid: u32,
	pub gid: u32,
	pub rdev: u32,
	pub blksize: u32,
	pub padding: u32,
}

impl fuse_attr {
	unsafe fn fill_stat(self, stat: *mut FileAttr) {
		(*stat).st_dev = 0;
		(*stat).st_ino = self.ino;
		(*stat).st_nlink = self.nlink as u64;
		(*stat).st_mode = self.mode;
		(*stat).st_uid = self.uid;
		(*stat).st_gid = self.gid;
		(*stat).st_rdev = self.rdev as u64;
		(*stat).st_size = self.size.try_into().unwrap();
		(*stat).st_blksize = self.blksize as i64;
		(*stat).st_blocks = self.blocks.try_into().unwrap();
		(*stat).st_atime = self.atime.try_into().unwrap();
		(*stat).st_atime_nsec = self.atimensec as i64;
		(*stat).st_mtime = self.mtime.try_into().unwrap();
		(*stat).st_mtime_nsec = self.atimensec as i64;
		(*stat).st_ctime = self.ctime.try_into().unwrap();
		(*stat).st_ctime_nsec = self.ctimensec as i64;
	}
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_unlink_in {}
unsafe impl FuseIn for fuse_unlink_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_unlink_out {}
unsafe impl FuseOut for fuse_unlink_out {}

fn create_unlink(name: &str) -> (Box<Cmd<fuse_unlink_in>>, Box<Rsp<fuse_unlink_out>>) {
	let slice = name.as_bytes();
	let len = core::mem::size_of::<fuse_in_header>()
		+ core::mem::size_of::<fuse_unlink_in>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_unlink_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw =
			core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1) as *mut Cmd<fuse_unlink_in>;
		(*raw).header = create_in_header::<fuse_unlink_in>(FUSE_ROOT_ID, Opcode::FUSE_UNLINK);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_unlink_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_unlink_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_unlink_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_create_in {
	pub flags: u32,
	pub mode: u32,
	pub umask: u32,
	pub open_flags: u32,
}
unsafe impl FuseIn for fuse_create_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_mkdir_in {
	pub mode: u32,
	pub umask: u32,
}
unsafe impl FuseIn for fuse_mkdir_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_create_out {
	pub entry: fuse_entry_out,
	pub open: fuse_open_out,
}

unsafe impl FuseOut for fuse_create_out {}

fn create_create(
	path: &str,
	flags: u32,
	mode: u32,
) -> (Box<Cmd<fuse_create_in>>, Box<Rsp<fuse_create_out>>) {
	let slice = path.as_bytes();
	let len = core::mem::size_of::<fuse_in_header>()
		+ core::mem::size_of::<fuse_create_in>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_create_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw =
			core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1) as *mut Cmd<fuse_create_in>;
		(*raw).header = create_in_header::<fuse_create_in>(FUSE_ROOT_ID, Opcode::FUSE_CREATE);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).cmd = fuse_create_in {
			flags,
			mode,
			..Default::default()
		};
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_create_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_create_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_create_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

fn create_mkdir(path: &str, mode: u32) -> (Box<Cmd<fuse_mkdir_in>>, Box<Rsp<fuse_entry_out>>) {
	let slice = path.as_bytes();
	let len = core::mem::size_of::<fuse_in_header>()
		+ core::mem::size_of::<fuse_mkdir_in>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_mkdir_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw =
			core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1) as *mut Cmd<fuse_mkdir_in>;
		(*raw).header = create_in_header::<fuse_mkdir_in>(FUSE_ROOT_ID, Opcode::FUSE_MKDIR);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).cmd = fuse_mkdir_in {
			mode,
			..Default::default()
		};
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_entry_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_entry_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_entry_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_rmdir_in {}
unsafe impl FuseIn for fuse_rmdir_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_rmdir_out {}
unsafe impl FuseOut for fuse_rmdir_out {}

fn create_rmdir(name: &str) -> (Box<Cmd<fuse_rmdir_in>>, Box<Rsp<fuse_rmdir_out>>) {
	let slice = name.as_bytes();
	let len = core::mem::size_of::<fuse_in_header>()
		+ core::mem::size_of::<fuse_rmdir_in>()
		+ slice.len()
		+ 1;
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_in_header>(),
			core::mem::align_of::<fuse_rmdir_in>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let cmd = unsafe {
		let data = alloc(layout);
		let raw =
			core::ptr::slice_from_raw_parts_mut(data, slice.len() + 1) as *mut Cmd<fuse_rmdir_in>;
		(*raw).header = create_in_header::<fuse_rmdir_in>(FUSE_ROOT_ID, Opcode::FUSE_RMDIR);
		(*raw).header.len = len.try_into().unwrap();
		(*raw).extra_buffer[..slice.len()].copy_from_slice(slice);
		(*raw).extra_buffer[slice.len()] = 0;

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*cmd));

	let len = core::mem::size_of::<fuse_out_header>() + core::mem::size_of::<fuse_rmdir_out>();
	let layout = Layout::from_size_align(
		len,
		core::cmp::max(
			core::mem::align_of::<fuse_out_header>(),
			core::mem::align_of::<fuse_rmdir_out>(),
		),
	)
	.unwrap()
	.pad_to_align();
	let rsp = unsafe {
		let data = alloc(layout);
		let raw = core::ptr::slice_from_raw_parts_mut(data, 0) as *mut Rsp<fuse_rmdir_out>;
		(*raw).header = fuse_out_header {
			len: len.try_into().unwrap(),
			..Default::default()
		};

		Box::from_raw(raw)
	};
	assert_eq!(layout, Layout::for_value(&*rsp));

	(cmd, rsp)
}

pub fn init() {
	if let Some(driver) = get_filesystem_driver() {
		// Instantiate global fuse object
		let fuse = Box::new(Fuse::new());
		fuse.send_init();

		let mut fs = fs::FILESYSTEM.lock();
		let mount_point = driver.lock().get_mount_point();
		info!("Mounting virtio-fs at /{}", mount_point);
		fs.mount(mount_point.as_str(), fuse)
			.expect("Mount failed. Duplicate mount_point?");
	}
}
