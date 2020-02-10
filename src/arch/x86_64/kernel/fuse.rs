use alloc::boxed::Box;
use alloc::rc::Rc;
use arch::x86_64::kernel::virtio::VirtiofsDriver;
use core::cell::RefCell;
use core::{fmt, u32, u8};
use synch::spinlock::Spinlock;

// response out layout eg @ https://github.com/zargony/fuse-rs/blob/bf6d1cf03f3277e35b580f3c7b9999255d72ecf3/src/ll/request.rs#L44

// TODO: remove this explicit dependency on virtiofs driver. (And make option for multiple fuse-fs?)
pub static FILESYSTEM: Spinlock<Option<Fuse<VirtiofsDriver>>> = Spinlock::new(None);

pub trait FuseInterface {
	fn send_command<S, T>(&mut self, cmd: Cmd<S>, rsp: Option<Rsp<T>>) -> Option<Rsp<T>>
	where
		S: FuseIn + core::fmt::Debug,
		T: FuseOut + core::fmt::Debug;
}

/*
pub struct FuseDriver {

}

impl FuseDriver {
	pub fn getDriver() -> Box<dyn FuseInterface> {

	}
}*/

pub struct Fuse<T: FuseInterface> {
	driver: Rc<RefCell<T>>,
}

/// Create global fuse object, store in FILESYSTEM
pub fn create_from_virtio(driver: Rc<RefCell<VirtiofsDriver<'static>>>) {
	let mut fs = FILESYSTEM.lock();
	if fs.is_some() {
		warn!("Replacing global FUSE object!");
	}
	let fuse = Fuse { driver };
	fs.replace(fuse);
}

impl<T: FuseInterface> Fuse<T> {
	pub fn send_hello(&self) {
		// TODO: this is a stack based buffer.. maybe not the best idea for DMA, but PoC works with this
		let (cmd, rsp) = create_init();
		let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
		info!("outside sdncmd {:?}", rsp);
	}

	pub fn lookup(&self, name: &str) -> u64 {
		let (cmd, rsp) = create_lookup(name);
		let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
		info!("outside sdncmd {:?}", rsp);
		rsp.unwrap().rsp.nodeid
	}

	pub fn open(&self, nid: u64) -> u64 {
		let (cmd, rsp) = create_open(nid);
		let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
		info!("outside sdncmd {:?}", rsp);
		rsp.unwrap().rsp.fh
	}

	pub fn read(&self, fh: u64) -> Box<[u8]> {
		let (cmd, rsp) = create_read(fh);
		let rsp = self.driver.borrow_mut().send_command(cmd, Some(rsp));
		info!("outside sdncmd {:?}", rsp);
		// TODO: do this zerocopy
		Box::new(rsp.unwrap().rsp.dat)
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
unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
	::core::slice::from_raw_parts((p as *const T) as *const u8, ::core::mem::size_of::<T>())
}
unsafe fn any_as_u8_slice_mut<T: Sized>(p: &mut T) -> &mut [u8] {
	::core::slice::from_raw_parts_mut((p as *mut T) as *mut u8, ::core::mem::size_of::<T>())
}

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
}

#[repr(C)]
#[derive(Debug)]
pub struct Rsp<T: FuseOut + core::fmt::Debug> {
	header: fuse_out_header,
	rsp: T,
}

// TODO: use from/into? But these require consuming the command, so we need some better memory model to avoid deallocation
impl<T> Cmd<T>
where
	T: FuseIn + core::fmt::Debug,
{
	pub fn to_u8buf(&self) -> &[u8] {
		unsafe { any_as_u8_slice(self) }
	}
}
impl<T> Rsp<T>
where
	T: FuseOut + core::fmt::Debug,
{
	pub fn to_u8buf_mut(&mut self) -> &mut [u8] {
		unsafe { &mut *any_as_u8_slice_mut(self) }
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
		},
		Rsp {
			rsp,
			header: rsphdr,
		},
	)
}

pub fn create_lookup(name: &str) -> (Cmd<fuse_lookup_in>, Rsp<fuse_entry_out>) {
	let cmd = name.into();
	let mut cmdhdr = create_in_header::<fuse_lookup_in>(Opcode::FUSE_LOOKUP);
	cmdhdr.nodeid = 1; // FUSE ROOT ID
	let rsp: fuse_entry_out = Default::default();
	let rsphdr: fuse_out_header = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
		},
		Rsp {
			rsp,
			header: rsphdr,
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
pub struct fuse_read_out {
	pub dat: [u8; 1024], // TODO: max read length?
}
unsafe impl FuseOut for fuse_read_out {}

impl fmt::Debug for fuse_read_out {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "fuse_read_out {{ {:?} }}", &self.dat[..])
	}
}

impl Default for fuse_read_out {
	fn default() -> Self {
		Self {
			dat: [0 as u8; 1024],
		}
	}
}

pub fn create_read(nid: u64) -> (Cmd<fuse_read_in>, Rsp<fuse_read_out>) {
	let cmd = Default::default();
	let mut cmdhdr = create_in_header::<fuse_open_in>(Opcode::FUSE_READ);
	cmdhdr.nodeid = nid;
	let rsp = Default::default();
	let rsphdr = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
		},
		Rsp {
			rsp,
			header: rsphdr,
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

pub fn create_open(nid: u64) -> (Cmd<fuse_open_in>, Rsp<fuse_open_out>) {
	let cmd = Default::default();
	let mut cmdhdr = create_in_header::<fuse_open_in>(Opcode::FUSE_OPEN);
	cmdhdr.nodeid = nid;
	let rsp = Default::default();
	let rsphdr = Default::default();
	(
		Cmd {
			cmd,
			header: cmdhdr,
		},
		Rsp {
			rsp,
			header: rsphdr,
		},
	)
}

#[repr(C)]
pub struct fuse_lookup_in {
	pub name: [u8; 256], // TODO: max path length?
}
unsafe impl FuseIn for fuse_lookup_in {}

impl From<&str> for fuse_lookup_in {
	fn from(name: &str) -> Self {
		let mut lookup = Self {
			name: [0 as u8; 256],
		};
		// TODO: fix this hacky conversion..
		for (i, c) in name.chars().enumerate() {
			lookup.name[i] = c as u8;
			if i > lookup.name.len() {
				warn!("FUSE: Name too long!");
				break;
			}
		}
		lookup
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
