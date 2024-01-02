#[cfg(all(feature = "fuse", feature = "pci"))]
pub(crate) mod fuse;
mod mem;
mod uhyve;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

use hermit_sync::OnceCell;
use mem::MemDirectory;

use crate::fd::{IoError, ObjectInterface, OpenOption};

pub(crate) static FILESYSTEM: OnceCell<Filesystem> = OnceCell::new();

/// Type of the VNode
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum NodeKind {
	/// Node represent a file
	File,
	/// Node represent a directory
	Directory,
}

/// VfsNode represents an internal node of the ramdisk.
pub(crate) trait VfsNode: core::fmt::Debug {
	/// Determines the current node type
	fn get_kind(&self) -> NodeKind;

	/// Helper function to create a new dirctory node
	fn traverse_mkdir(&self, _components: &mut Vec<&str>, _mode: u32) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to delete a dirctory node
	fn traverse_rmdir(&self, _components: &mut Vec<&str>) -> core::result::Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to remove the specified file
	fn traverse_unlink(&self, _components: &mut Vec<&str>) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to open a directory
	fn traverse_opendir(
		&self,
		_components: &mut Vec<&str>,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to get file status
	fn traverse_lstat(&self, _components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to get file status
	fn traverse_stat(&self, _components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to mount a file system
	fn traverse_mount(
		&self,
		_components: &mut Vec<&str>,
		_obj: Box<dyn VfsNode + core::marker::Send + core::marker::Sync>,
	) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to open a file
	fn traverse_open(
		&self,
		_components: &mut Vec<&str>,
		_option: OpenOption,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Err(IoError::ENOSYS)
	}
}

#[derive(Debug)]
pub(crate) struct Filesystem {
	root: MemDirectory,
}

impl Filesystem {
	pub fn new() -> Self {
		Self {
			root: MemDirectory::new(),
		}
	}

	/// Tries to open file at given path.
	pub fn open(&self, path: &str, opt: OpenOption) -> Result<Arc<dyn ObjectInterface>, IoError> {
		info!("Open file {}", path);
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_open(&mut components, opt)
	}

	/// Unlinks a file given by path
	pub fn unlink(&self, path: &str) -> Result<(), IoError> {
		debug!("Unlinking file {}", path);
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_unlink(&mut components)
	}

	/// Remove directory given by path
	pub fn rmdir(&self, path: &str) -> Result<(), IoError> {
		debug!("Removing directory {}", path);
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_rmdir(&mut components)
	}

	/// Create directory given by path
	pub fn mkdir(&self, path: &str, mode: u32) -> Result<(), IoError> {
		debug!("Create directory {}", path);
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_mkdir(&mut components, mode)
	}

	/// List given directory
	pub fn opendir(&self, path: &str) -> Result<Arc<dyn ObjectInterface>, IoError> {
		if path.trim() == "/" {
			let mut components: Vec<&str> = Vec::new();
			self.root.traverse_opendir(&mut components)
		} else {
			let mut components: Vec<&str> = path.split('/').collect();

			components.reverse();
			components.pop();

			self.root.traverse_opendir(&mut components)
		}
	}

	/// stat
	pub fn stat(&self, path: &str) -> Result<FileAttr, IoError> {
		debug!("Getting stats {}", path);

		let mut components: Vec<&str> = path.split('/').collect();
		components.reverse();
		components.pop();

		self.root.traverse_stat(&mut components)
	}

	/// lstat
	pub fn lstat(&self, path: &str) -> Result<FileAttr, IoError> {
		debug!("Getting lstats {}", path);

		let mut components: Vec<&str> = path.split('/').collect();
		components.reverse();
		components.pop();

		self.root.traverse_lstat(&mut components)
	}

	/// Create new backing-fs at mountpoint mntpath
	pub fn mount(
		&self,
		path: &str,
		obj: Box<dyn VfsNode + core::marker::Send + core::marker::Sync>,
	) -> Result<(), IoError> {
		debug!("Mounting {}", path);

		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_mount(&mut components, obj)
	}

	/// Create file from ROM
	pub unsafe fn create_file(
		&self,
		name: &str,
		ptr: *const u8,
		length: usize,
	) -> Result<(), IoError> {
		self.root.create_file(name, ptr, length)
	}
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct FileAttr {
	pub st_dev: u64,
	pub st_ino: u64,
	pub st_nlink: u64,
	pub st_mode: u32,
	pub st_uid: u32,
	pub st_gid: u32,
	pub st_rdev: u64,
	pub st_size: i64,
	pub st_blksize: i64,
	pub st_blocks: i64,
	pub st_atime: i64,
	pub st_atime_nsec: i64,
	pub st_mtime: i64,
	pub st_mtime_nsec: i64,
	pub st_ctime: i64,
	pub st_ctime_nsec: i64,
}

#[derive(Debug, FromPrimitive, ToPrimitive)]
pub enum FileType {
	Unknown = 0,         // DT_UNKNOWN
	Fifo = 1,            // DT_FIFO
	CharacterDevice = 2, // DT_CHR
	Directory = 4,       // DT_DIR
	BlockDevice = 6,     // DT_BLK
	RegularFile = 8,     // DT_REG
	SymbolicLink = 10,   // DT_LNK
	Socket = 12,         // DT_SOCK
	Whiteout = 14,       // DT_WHT
}

#[derive(Debug, FromPrimitive, ToPrimitive)]
pub enum SeekWhence {
	Set = 0,
	Cur = 1,
	End = 2,
	Data = 3,
	Hole = 4,
}

pub(crate) fn init() {
	FILESYSTEM.set(Filesystem::new()).unwrap();
	FILESYSTEM
		.get()
		.unwrap()
		.mkdir("/tmp", 777)
		.expect("Unable to create /tmp");

	#[cfg(all(feature = "fuse", feature = "pci"))]
	fuse::init();
	uhyve::init();
}

pub unsafe fn create_file(name: &str, ptr: *const u8, length: usize) {
	unsafe {
		FILESYSTEM
			.get()
			.unwrap()
			.create_file(name, ptr, length)
			.expect("Unable to create file from ROM")
	}
}
