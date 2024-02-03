pub(super) const FUSE_ROOT_ID: u64 = 1;

#[allow(dead_code)]
pub(super) const FUSE_GETATTR_FH: u32 = 1 << 0;

#[repr(C)]
#[derive(Debug)]
pub(super) struct fuse_dirent {
	pub d_ino: u64,
	pub d_off: u64,
	pub d_namelen: u32,
	pub d_type: u32,
	pub d_name: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct fuse_in_header {
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
pub(super) struct fuse_out_header {
	pub len: u32,
	pub error: i32,
	pub unique: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct fuse_init_in {
	pub major: u32,
	pub minor: u32,
	pub max_readahead: u32,
	pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub(super) struct fuse_init_out {
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
pub(super) struct fuse_read_in {
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
pub struct fuse_write_in {
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
pub(super) struct fuse_write_out {
	pub size: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_read_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_lookup_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_readlink_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub struct fuse_readlink_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_attr_out {
	pub attr_valid: u64,
	pub attr_valid_nsec: u32,
	pub dummy: u32,
	pub attr: fuse_attr,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_entry_out {
	pub nodeid: u64,
	pub generation: u64,
	pub entry_valid: u64,
	pub attr_valid: u64,
	pub entry_valid_nsec: u32,
	pub attr_valid_nsec: u32,
	pub attr: fuse_attr,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_attr {
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
pub(super) struct fuse_create_in {
	pub flags: u32,
	pub mode: u32,
	pub umask: u32,
	pub open_flags: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_create_out {
	pub entry: fuse_entry_out,
	pub open: fuse_open_out,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_open_in {
	pub flags: u32,
	pub unused: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_open_out {
	pub fh: u64,
	pub open_flags: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_release_in {
	pub fh: u64,
	pub flags: u32,
	pub release_flags: u32,
	pub lock_owner: u64,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_release_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_rmdir_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_rmdir_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_mkdir_in {
	pub mode: u32,
	pub umask: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_unlink_in {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_unlink_out {}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_lseek_in {
	pub fh: u64,
	pub offset: u64,
	pub whence: u32,
	pub padding: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_lseek_out {
	pub(super) offset: u64,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_poll_in {
	pub fh: u64,
	pub kh: u64,
	pub flags: u32,
	pub events: u32,
}

#[repr(C)]
#[derive(Default, Debug)]
pub(super) struct fuse_poll_out {
	pub revents: u32,
	padding: u32,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub(super) enum Opcode {
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
