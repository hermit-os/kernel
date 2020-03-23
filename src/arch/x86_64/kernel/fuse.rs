use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::{fmt, u32, u8};
use syscalls::fs::{FileError, FilePerms, PosixFile, PosixFileSystem, SeekWhence};

// response out layout eg @ https://github.com/zargony/fuse-rs/blob/bf6d1cf03f3277e35b580f3c7b9999255d72ecf3/src/ll/request.rs#L44
// op in/out sizes/layout: https://github.com/hanwen/go-fuse/blob/204b45dba899dfa147235c255908236d5fde2d32/fuse/opcode.go#L439
// possible reponses for command: qemu/tools/virtiofsd/fuse_lowlevel.h

const FUSE_ROOT_ID: u64 = 1;
const MAX_READ_LEN: usize = 1024 * 64;
const MAX_WRITE_LEN: usize = 1024 * 64;

pub trait FuseInterface {
	fn send_command<S, T>(&mut self, cmd: Cmd<S>, rsp: Option<Rsp<T>>) -> Option<Rsp<T>>
	where
		S: FuseIn + core::fmt::Debug,
		T: FuseOut + core::fmt::Debug;
}

pub struct Fuse<T: FuseInterface> {
	driver: Rc<RefCell<T>>,
}

impl<T: FuseInterface + 'static> PosixFileSystem for Fuse<T> {
	fn open(&self, path: &str, perms: FilePerms) -> Result<Box<dyn PosixFile>, FileError> {
		let mut file = FuseFile {
			driver: self.driver.clone(),
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

			if file.fuse_nid == None {
				warn!("Lookup seems to have failed!");
				return Err(FileError::ENOENT());
			}

			// 3.FUSE_OPEN(nodeid, O_RDONLY) -> fh
			let (cmd, rsp) = create_open(file.fuse_nid.unwrap(), perms.raw);
			let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
			debug!("Open answer {:?}", rsp);
			file.fuse_fh = Some(rsp.unwrap().rsp.fh);
		} else {
			// Create file (opens implicitly, returns results from both lookup and open calls)
			let (cmd, rsp) = create_create(path, perms.raw, perms.mode);
			let rsp = self
				.driver
				.borrow_mut()
				.send_command(cmd, Some(rsp))
				.unwrap();
			debug!("Create answer {:?}", rsp);

			file.fuse_nid = Some(rsp.rsp.entry.nodeid);
			file.fuse_fh = Some(rsp.rsp.open.fh);
		}

		Ok(Box::new(file))
	}

	fn unlink(&self, path: &str) -> core::result::Result<(), FileError> {
		let (cmd, rsp) = create_unlink(path);
		let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
		debug!("unlink answer {:?}", rsp);

		Ok(())
	}
}

impl<T: FuseInterface + 'static> Fuse<T> {
	pub fn new(driver: Rc<RefCell<T>>) -> Self {
		Self { driver }
	}

	pub fn send_init(&self) {
		let (cmd, rsp) = create_init();
		let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
		debug!("fuse init answer: {:?}", rsp);
	}

	pub fn lookup(&self, name: &str) -> Option<u64> {
		let (cmd, rsp) = create_lookup(name);
		let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
		Some(rsp.unwrap().rsp.nodeid)
	}
}

struct FuseFile<T: FuseInterface> {
	driver: Rc<RefCell<T>>,
	fuse_nid: Option<u64>,
	fuse_fh: Option<u64>,
	offset: usize,
}

impl<T: FuseInterface> PosixFile for FuseFile<T> {
	fn close(&mut self) -> Result<(), FileError> {
		let (cmd, rsp) = create_release(self.fuse_nid.unwrap(), self.fuse_fh.unwrap());
		self.driver.borrow_mut().send_command(cmd, Some(rsp));

		Ok(())
	}

	fn read(&mut self, len: u32) -> Result<Vec<u8>, FileError> {
		let mut len = len;
		if len as usize > MAX_READ_LEN {
			info!("Reading longer than max_read_len: {}", len);
			len = MAX_READ_LEN as u32;
		}
		if let Some(fh) = self.fuse_fh {
			let (cmd, rsp) = create_read(fh, len, self.offset as u64);
			let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
			let rsp = rsp.unwrap();
			let len = rsp.header.len as usize - ::core::mem::size_of::<fuse_out_header>();
			self.offset += len;
			// TODO: do this zerocopy
			let mut vec = rsp.extra_buffer.unwrap();
			vec.truncate(len);
			info!("LEN: {}, VEC: {:?}", len, vec);
			Ok(vec)
		} else {
			warn!("File not open, cannot read!");
			Err(FileError::ENOENT())
		}
	}

	fn write(&mut self, buf: &[u8]) -> Result<u64, FileError> {
		info!("fuse write!");
		let mut len = buf.len();
		if len as usize > MAX_WRITE_LEN {
			debug!(
				"Writing longer than max_write_len: {} > {}",
				buf.len(),
				MAX_WRITE_LEN
			);
			len = MAX_WRITE_LEN;
		}
		if let Some(fh) = self.fuse_fh {
			let (cmd, rsp) = create_write(fh, &buf[..len], self.offset as u64);
			let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
			info!("write response: {:?}", rsp);
			let rsp = rsp.unwrap();

			let len = rsp.rsp.size as usize;
			self.offset += len;
			info!("Written {} bytes", len);
			Ok(len as u64)
		} else {
			warn!("File not open, cannot read!");
			Err(FileError::ENOENT())
		}
	}

	fn lseek(&mut self, offset: isize, whence: SeekWhence) -> Result<usize, FileError> {
		info!("fuse lseek");

		match whence {
			SeekWhence::Set => self.offset = offset as usize,
			SeekWhence::Cur => self.offset = (self.offset as isize + offset) as usize,
			SeekWhence::End => unimplemented!("Cant seek from end yet!"),
		}

		Ok(self.offset)
	}
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
#[allow(non_camel_case_types)]
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

	FUSE_SETVOLNAME = 61,
	FUSE_GETXTIMES = 62,
	FUSE_EXCHANGE = 63,

	CUSE_INIT = 4096,
}

// From https://stackoverflow.com/questions/28127165/how-to-convert-struct-to-u8
/*unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
	::core::slice::from_raw_parts(
		(p as *const T) as *const u8,
		::core::mem::size_of::<T>(),
	)
}
unsafe fn any_as_u8_slice_mut<T: Sized>(p: &mut T) -> &mut [u8] {
	::core::slice::from_raw_parts_mut(
		(p as *mut T) as *mut u8,
		::core::mem::size_of::<T>(),
	)
}*/

/// Marker trait, which signals that a struct is a valid Fuse command.
/// Struct has to be repr(C)!
pub unsafe trait FuseIn {}
/// Marker trait, which signals that a struct is a valid Fuse response.
/// Struct has to be repr(C)!
pub unsafe trait FuseOut {}

#[repr(C)]
#[derive(Debug)]
pub struct Cmd<T: FuseIn + core::fmt::Debug> {
	header: fuse_in_header,
	cmd: T,
	extra_buffer: Option<Vec<u8>>, // eg for writes. allows zero-copy and avoids rust size_of operations (which always add alignment padding)
}

#[repr(C)]
#[derive(Debug)]
pub struct Rsp<T: FuseOut + core::fmt::Debug> {
	header: fuse_out_header,
	rsp: T,
	extra_buffer: Option<Vec<u8>>, // eg for reads. allows zero-copy and avoids rust size_of operations (which always add alignment padding)
}

// TODO: use from/into? But these require consuming the command, so we need some better memory model to avoid deallocation
impl<T> Cmd<T>
where
	T: FuseIn + core::fmt::Debug,
{
	pub fn to_u8buf(&self) -> Vec<&[u8]> {
		let rawcmd = unsafe {
			::core::slice::from_raw_parts(
				(&self.header as *const fuse_in_header) as *const u8,
				::core::mem::size_of::<T>() + ::core::mem::size_of::<fuse_in_header>(),
			)
		};
		if let Some(extra) = &self.extra_buffer {
			vec![rawcmd, &extra.as_ref()]
		} else {
			vec![rawcmd]
		}
	}
}
impl<T> Rsp<T>
where
	T: FuseOut + core::fmt::Debug,
{
	pub fn to_u8buf_mut(&mut self) -> Vec<&mut [u8]> {
		let rawrsp = unsafe {
			::core::slice::from_raw_parts_mut(
				(&mut self.header as *mut fuse_out_header) as *mut u8,
				::core::mem::size_of::<T>() + ::core::mem::size_of::<fuse_out_header>(),
			)
		};
		if let Some(extra) = self.extra_buffer.as_mut() {
			vec![rawrsp, extra]
		} else {
			vec![rawrsp]
		}
	}
}

pub fn create_in_header<T>(opcode: Opcode) -> fuse_in_header
where
	T: FuseIn,
{
	fuse_in_header {
		len: (core::mem::size_of::<T>() + core::mem::size_of::<T>()) as u32,
		opcode: opcode as u32,
		unique: 1,
		nodeid: 0,
		uid: 0,
		pid: 0,
		gid: 0,
		padding: 0,
	}
}

pub fn create_init() -> (Cmd<fuse_init_in>, Rsp<fuse_init_out>) {
	let cmd = fuse_init_in {
		major: 7,
		minor: 31,
		max_readahead: 0,
		flags: 0,
	};
	let cmdhdr = create_in_header::<fuse_init_in>(Opcode::FUSE_INIT);
	let rsp: fuse_init_out = Default::default();
	let rsphdr: fuse_out_header = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: None,
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: None,
		},
	)
}

pub fn create_lookup(name: &str) -> (Cmd<fuse_lookup_in>, Rsp<fuse_entry_out>) {
	let cmd = name.into();
	let mut cmdhdr = create_in_header::<fuse_lookup_in>(Opcode::FUSE_LOOKUP);
	cmdhdr.nodeid = FUSE_ROOT_ID;
	let rsp: fuse_entry_out = Default::default();
	let rsphdr: fuse_out_header = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: None,
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: None,
		},
	)
}

#[repr(C)]
#[derive(Debug)]
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
#[derive(Debug, Default)]
pub struct fuse_out_header {
	pub len: u32,
	pub error: i32,
	pub unique: u64,
}

#[repr(C)]
#[derive(Debug)]
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

pub fn create_read(nid: u64, size: u32, offset: u64) -> (Cmd<fuse_read_in>, Rsp<fuse_read_out>) {
	let cmd = fuse_read_in {
		offset,
		size,
		..Default::default()
	};
	let mut cmdhdr = create_in_header::<fuse_read_in>(Opcode::FUSE_READ);
	cmdhdr.nodeid = nid;
	let rsp = Default::default();
	let rsphdr = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: None,
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: Some(vec![0; size as usize]),
		},
	)
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

// TODO: do write zerocopy? currently does buf.to_vec()
pub fn create_write(
	nid: u64,
	buf: &[u8],
	offset: u64,
) -> (Cmd<fuse_write_in>, Rsp<fuse_write_out>) {
	let cmd = fuse_write_in {
		offset,
		size: buf.len() as u32,
		..Default::default()
	};
	let mut cmdhdr = create_in_header::<fuse_write_in>(Opcode::FUSE_WRITE);
	cmdhdr.nodeid = nid;
	let rsp = Default::default();
	let rsphdr = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: Some(buf.to_vec()),
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: None,
		},
	)
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

pub fn create_open(nid: u64, flags: u32) -> (Cmd<fuse_open_in>, Rsp<fuse_open_out>) {
	let cmd = fuse_open_in {
		flags,
		..Default::default()
	};
	let mut cmdhdr = create_in_header::<fuse_open_in>(Opcode::FUSE_OPEN);
	cmdhdr.nodeid = nid;
	let rsp = Default::default();
	let rsphdr = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: None,
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: None,
		},
	)
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

pub fn create_release(nid: u64, fh: u64) -> (Cmd<fuse_release_in>, Rsp<fuse_release_out>) {
	let mut cmd: fuse_release_in = Default::default();
	let mut cmdhdr = create_in_header::<fuse_release_in>(Opcode::FUSE_RELEASE);
	cmdhdr.nodeid = nid;
	cmd.fh = fh;
	let rsp = Default::default();
	let rsphdr = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: None,
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: None,
		},
	)
}

fn str_into_u8buf(s: &str, u8buf: &mut [u8]) {
	// TODO: fix this hacky conversion..
	for (i, c) in s.chars().enumerate() {
		u8buf[i] = c as u8;
		if i > u8buf.len() {
			warn!("FUSE: Name too long!");
			break;
		}
	}
}

// TODO: max path length?
const MAX_PATH_LEN: usize = 256;
fn str_to_path(s: &str) -> [u8; MAX_PATH_LEN] {
	let mut buf = [0 as u8; MAX_PATH_LEN];
	str_into_u8buf(s, &mut buf);
	buf
}

#[repr(C)]
pub struct fuse_lookup_in {
	pub name: [u8; MAX_PATH_LEN],
}
unsafe impl FuseIn for fuse_lookup_in {}

impl From<&str> for fuse_lookup_in {
	fn from(name: &str) -> Self {
		Self {
			name: str_to_path(name),
		}
	}
}

impl fmt::Debug for fuse_lookup_in {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "fuse_lookup_in {{ {:?} }}", &self.name[..])
	}
}

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

#[repr(C)]
pub struct fuse_unlink_in {
	pub name: [u8; MAX_PATH_LEN],
}
unsafe impl FuseIn for fuse_unlink_in {}

impl From<&str> for fuse_unlink_in {
	fn from(name: &str) -> Self {
		Self {
			name: str_to_path(name),
		}
	}
}

impl fmt::Debug for fuse_unlink_in {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "fuse_unlink_in {{ {:?} }}", &self.name[..])
	}
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_unlink_out {}
unsafe impl FuseOut for fuse_unlink_out {}

pub fn create_unlink(name: &str) -> (Cmd<fuse_unlink_in>, Rsp<fuse_unlink_out>) {
	let cmd = name.into();
	let mut cmdhdr = create_in_header::<fuse_unlink_in>(Opcode::FUSE_UNLINK);
	cmdhdr.nodeid = FUSE_ROOT_ID;
	let rsp: fuse_unlink_out = Default::default();
	let rsphdr: fuse_out_header = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: None,
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: None,
		},
	)
}

#[repr(C)]
pub struct fuse_create_in {
	pub flags: u32,
	pub mode: u32,
	pub umask: u32,
	pub padding: u32,
	pub name: [u8; MAX_PATH_LEN],
}
unsafe impl FuseIn for fuse_create_in {}

#[repr(C)]
#[derive(Debug, Default)]
pub struct fuse_create_out {
	pub entry: fuse_entry_out,
	pub open: fuse_open_out,
}
unsafe impl FuseOut for fuse_create_out {}

impl fuse_create_in {
	fn new(name: &str, flags: u32, mode: u32) -> Self {
		Self {
			flags,
			mode,
			umask: 0,
			padding: 0,
			name: str_to_path(name),
		}
	}
}

impl fmt::Debug for fuse_create_in {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"fuse_create_in {{ flags: {}, mode: {}, umask: {}, name: {:?} ...}}",
			self.flags,
			self.mode,
			self.umask,
			&self.name[..10]
		)
	}
}

pub fn create_create(
	path: &str,
	flags: u32,
	mode: u32,
) -> (Cmd<fuse_create_in>, Rsp<fuse_create_out>) {
	let cmd = fuse_create_in::new(path, flags, mode);
	let mut cmdhdr = create_in_header::<fuse_create_in>(Opcode::FUSE_CREATE);
	cmdhdr.nodeid = FUSE_ROOT_ID;
	let rsp = Default::default();
	let rsphdr = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
			extra_buffer: None,
		},
		Rsp {
			rsp,
			header: rsphdr,
			extra_buffer: None,
		},
	)
}
