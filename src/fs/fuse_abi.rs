#![allow(dead_code)]

#[cfg(all(feature = "fuse", feature = "pci"))]
pub(crate) const ROOT_ID: u64 = 1;

#[allow(dead_code)]
pub(crate) const GETATTR_FH: u32 = 1 << 0;

#[repr(C)]
#[derive(Debug)]
pub(crate) struct Dirent {
	pub d_ino: u64,
	pub d_off: u64,
	pub d_namelen: u32,
	pub d_type: u32,
	pub d_name: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct InHeader {
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
pub(crate) struct OutHeader {
	pub len: u32,
	pub error: i32,
	pub unique: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct InitIn {
	pub major: u32,
	pub minor: u32,
	pub max_readahead: u32,
	pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct InitOut {
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

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct ReadIn {
	pub fh: u64,
	pub offset: u64,
	pub size: u32,
	pub read_flags: u32,
	pub lock_owner: u64,
	pub flags: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct WriteIn {
	pub fh: u64,
	pub offset: u64,
	pub size: u32,
	pub write_flags: u32,
	pub lock_owner: u64,
	pub flags: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct WriteOut {
	pub size: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct ReadOut {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct LookupIn {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct ReadlinkIn {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct ReadlinkOut {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct AttrOut {
	pub attr_valid: u64,
	pub attr_valid_nsec: u32,
	pub dummy: u32,
	pub attr: Attr,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct EntryOut {
	pub nodeid: u64,
	pub generation: u64,
	pub entry_valid: u64,
	pub attr_valid: u64,
	pub entry_valid_nsec: u32,
	pub attr_valid_nsec: u32,
	pub attr: Attr,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct Attr {
	/// inode number
	pub ino: u64,
	/// size in bytes
	pub size: u64,
	/// size in blocks
	pub blocks: u64,
	/// time of last access
	pub atime: u64,
	/// time of last modification
	pub mtime: u64,
	/// time of last status change
	pub ctime: u64,
	pub atimensec: u32,
	pub mtimensec: u32,
	pub ctimensec: u32,
	/// access permissions
	pub mode: u32,
	/// number of hard links
	pub nlink: u32,
	/// user id
	pub uid: u32,
	/// group id
	pub gid: u32,
	/// device id
	pub rdev: u32,
	/// block size
	pub blksize: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct CreateIn {
	pub flags: u32,
	pub mode: u32,
	pub umask: u32,
	pub open_flags: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct CreateOut {
	pub entry: EntryOut,
	pub open: OpenOut,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct OpenIn {
	pub flags: u32,
	pub unused: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct OpenOut {
	pub fh: u64,
	pub open_flags: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct ReleaseIn {
	pub fh: u64,
	pub flags: u32,
	pub release_flags: u32,
	pub lock_owner: u64,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct ReleaseOut {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct RmdirIn {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct RmdirOut {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct MkdirIn {
	pub mode: u32,
	pub umask: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct UnlinkIn {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct UnlinkOut {}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct LseekIn {
	pub fh: u64,
	pub offset: u64,
	pub whence: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct LseekOut {
	pub(crate) offset: u64,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct PollIn {
	pub fh: u64,
	pub kh: u64,
	pub flags: u32,
	pub events: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(crate) struct PollOut {
	pub revents: u32,
	padding: u32,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone)]
#[allow(dead_code)]
pub(crate) enum Opcode {
	Lookup = 1,
	Forget = 2, // no reply
	Getattr = 3,
	Setattr = 4,
	Readlink = 5,
	Symlink = 6,
	Mknod = 8,
	Mkdir = 9,
	Unlink = 10,
	Rmdir = 11,
	Rename = 12,
	Link = 13,
	Open = 14,
	Read = 15,
	Write = 16,
	Statfs = 17,
	Release = 18,
	Fsync = 20,
	Setxattr = 21,
	Getxattr = 22,
	Listxattr = 23,
	Removexattr = 24,
	Flush = 25,
	Init = 26,
	Opendir = 27,
	Readdir = 28,
	Releasedir = 29,
	Fsyncdir = 30,
	Getlk = 31,
	Setlk = 32,
	Setlkw = 33,
	Access = 34,
	Create = 35,
	Interrupt = 36,
	Bmap = 37,
	Destroy = 38,
	Ioctl = 39,
	Poll = 40,
	NotifyReply = 41,
	BatchForget = 42,
	Fallocate = 43,
	Readdirplus = 44,
	Rename2 = 45,
	Lseek = 46,

	Setvolname = 61,
	Getxtimes = 62,
	Exchange = 63,

	CuseInit = 4096,
}
