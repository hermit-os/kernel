#[cfg(all(feature = "fuse", feature = "pci"))]
pub(crate) mod fuse;
mod mem;
mod uhyve;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

use hermit_sync::OnceCell;
use mem::MemDirectory;

use crate::fd::{
	insert_object, AccessPermission, IoError, ObjectInterface, OpenOption, FD_COUNTER,
};
use crate::io::Write;

pub(crate) static FILESYSTEM: OnceCell<Filesystem> = OnceCell::new();

pub const MAX_NAME_LENGTH: usize = 256;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DirectoryEntry {
	pub d_name: [u8; MAX_NAME_LENGTH],
}

impl DirectoryEntry {
	pub fn new(d_name: &[u8]) -> Self {
		let len = core::cmp::min(d_name.len(), MAX_NAME_LENGTH);
		let mut entry = Self {
			d_name: [0; MAX_NAME_LENGTH],
		};

		entry.d_name[..len].copy_from_slice(&d_name[..len]);

		entry
	}
}

impl fmt::Debug for DirectoryEntry {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let d_name = unsafe { CStr::from_ptr(self.d_name.as_ptr() as _) }
			.to_str()
			.unwrap();

		f.debug_struct("DirectoryEntry")
			.field("d_name", &d_name)
			.finish()
	}
}

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

	/// determines the current file attribute
	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		Err(IoError::ENOSYS)
	}

	/// Determine the syscall interface
	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Err(IoError::ENOSYS)
	}

	/// Helper function to create a new dirctory node
	fn traverse_mkdir(
		&self,
		_components: &mut Vec<&str>,
		_mode: AccessPermission,
	) -> Result<(), IoError> {
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
	fn traverse_readdir(
		&self,
		_components: &mut Vec<&str>,
	) -> Result<Vec<DirectoryEntry>, IoError> {
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
		_mode: AccessPermission,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Err(IoError::ENOSYS)
	}
}

#[derive(Debug)]
struct DirectoryReader {
	pos: AtomicUsize,
	data: Vec<DirectoryEntry>,
}

impl DirectoryReader {
	pub fn new(data: Vec<DirectoryEntry>) -> Self {
		Self {
			pos: AtomicUsize::new(0),
			data,
		}
	}
}

impl ObjectInterface for DirectoryReader {
	fn readdir(&self) -> Result<Option<DirectoryEntry>, IoError> {
		let pos = self.pos.fetch_add(1, Ordering::SeqCst);
		if pos < self.data.len() {
			Ok(Some(self.data[pos]))
		} else {
			Ok(None)
		}
	}
}

impl Clone for DirectoryReader {
	fn clone(&self) -> Self {
		Self {
			pos: AtomicUsize::new(self.pos.load(Ordering::SeqCst)),
			data: self.data.clone(),
		}
	}
}

#[derive(Debug)]
pub(crate) struct Filesystem {
	root: MemDirectory,
}

impl Filesystem {
	pub fn new() -> Self {
		Self {
			root: MemDirectory::new(AccessPermission::from_bits(0o777).unwrap()),
		}
	}

	/// Tries to open file at given path.
	pub fn open(
		&self,
		path: &str,
		opt: OpenOption,
		mode: AccessPermission,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		debug!("Open file {} with {:?}", path, opt);
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_open(&mut components, opt, mode)
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
	pub fn mkdir(&self, path: &str, mode: AccessPermission) -> Result<(), IoError> {
		debug!("Create directory {}", path);
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_mkdir(&mut components, mode)
	}

	pub fn opendir(&self, path: &str) -> Result<Arc<dyn ObjectInterface>, IoError> {
		debug!("Open directory {}", path);
		Ok(Arc::new(DirectoryReader::new(self.readdir(path)?)))
	}

	/// List given directory
	pub fn readdir(&self, path: &str) -> Result<Vec<DirectoryEntry>, IoError> {
		if path.trim() == "/" {
			let mut components: Vec<&str> = Vec::new();
			self.root.traverse_readdir(&mut components)
		} else {
			let mut components: Vec<&str> = path.split('/').collect();

			components.reverse();
			components.pop();

			self.root.traverse_readdir(&mut components)
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
		mode: AccessPermission,
	) -> Result<(), IoError> {
		self.root.create_file(name, ptr, length, mode)
	}
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct FileAttr {
	pub st_dev: u64,
	pub st_ino: u64,
	pub st_nlink: u64,
	pub st_mode: AccessPermission,
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
	const VERSION: &str = env!("CARGO_PKG_VERSION");
	const UTC_BUILT_TIME: &str = build_time::build_time_utc!();

	FILESYSTEM.set(Filesystem::new()).unwrap();
	FILESYSTEM
		.get()
		.unwrap()
		.mkdir("/tmp", AccessPermission::from_bits(0o777).unwrap())
		.expect("Unable to create /tmp");
	FILESYSTEM
		.get()
		.unwrap()
		.mkdir("/proc", AccessPermission::from_bits(0o777).unwrap())
		.expect("Unable to create /proc");

	if let Ok(mut file) = File::create("/proc/version") {
		if write!(file, "HermitOS version {VERSION} # UTC {UTC_BUILT_TIME}").is_err() {
			error!("Unable to write in /proc/version");
		}
	} else {
		error!("Unable to create /proc/version");
	}

	#[cfg(all(feature = "fuse", feature = "pci"))]
	fuse::init();
	uhyve::init();
}

pub unsafe fn create_file(
	name: &str,
	ptr: *const u8,
	length: usize,
	mode: AccessPermission,
) -> Result<(), IoError> {
	unsafe {
		FILESYSTEM
			.get()
			.ok_or(IoError::EINVAL)?
			.create_file(name, ptr, length, mode)
	}
}

/// Returns an vectri with all the entries within a directory.
pub fn readdir(name: &str) -> Result<Vec<DirectoryEntry>, IoError> {
	debug!("Read directory {}", name);

	FILESYSTEM.get().ok_or(IoError::EINVAL)?.readdir(name)
}

/// a
pub(crate) fn opendir(name: &str) -> Result<FileDescriptor, IoError> {
	let obj = FILESYSTEM.get().unwrap().opendir(name)?;
	let fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);

	let _ = insert_object(fd, obj);

	Ok(fd)
}

use crate::fd::{self, FileDescriptor};

pub fn file_attributes(path: &str) -> Result<FileAttr, IoError> {
	FILESYSTEM.get().unwrap().lstat(path)
}

#[derive(Debug)]
pub struct File(FileDescriptor);

impl File {
	/// Creates a new file in read-write mode; error if the file exists.
	///
	/// This function will create a file if it does not exist, or return
	/// an error if it does. This way, if the call succeeds, the file
	/// returned is guaranteed to be new.
	pub fn create(path: &str) -> Result<Self, IoError> {
		let fd = fd::open(
			path,
			OpenOption::O_CREAT | OpenOption::O_RDWR,
			AccessPermission::from_bits(0o666).unwrap(),
		)?;

		Ok(File(fd))
	}

	/// Attempts to open a file in read-write mode.
	pub fn open(path: &str) -> Result<Self, IoError> {
		let fd = fd::open(
			path,
			OpenOption::O_RDWR,
			AccessPermission::from_bits(0o666).unwrap(),
		)?;

		Ok(File(fd))
	}
}

impl crate::io::Read for File {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
		fd::read(self.0, buf)
	}
}

impl crate::io::Write for File {
	fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
		fd::write(self.0, buf)
	}
}

impl Drop for File {
	fn drop(&mut self) {
		fd::close(self.0);
	}
}
