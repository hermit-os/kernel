#[cfg(all(feature = "fuse", feature = "pci"))]
pub(crate) mod fuse;
mod mem;
mod uhyve;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use async_trait::async_trait;
use hermit_sync::OnceCell;
use mem::MemDirectory;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::fd::{AccessPermission, ObjectInterface, OpenOption, insert_object, remove_object};
use crate::io;
use crate::io::Write;
use crate::time::{SystemTime, timespec};

static FILESYSTEM: OnceCell<Filesystem> = OnceCell::new();

#[derive(Debug, Clone)]
pub struct DirectoryEntry {
	pub name: String,
}

impl DirectoryEntry {
	pub fn new(name: String) -> Self {
		Self { name }
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
	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		Err(io::Error::ENOSYS)
	}

	/// Determine the syscall interface
	fn get_object(&self) -> io::Result<Arc<dyn ObjectInterface>> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to create a new directory node
	fn traverse_mkdir(
		&self,
		_components: &mut Vec<&str>,
		_mode: AccessPermission,
	) -> io::Result<()> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to delete a directory node
	fn traverse_rmdir(&self, _components: &mut Vec<&str>) -> io::Result<()> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to remove the specified file
	fn traverse_unlink(&self, _components: &mut Vec<&str>) -> io::Result<()> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to open a directory
	fn traverse_readdir(&self, _components: &mut Vec<&str>) -> io::Result<Vec<DirectoryEntry>> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to get file status
	fn traverse_lstat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to get file status
	fn traverse_stat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to mount a file system
	fn traverse_mount(
		&self,
		_components: &mut Vec<&str>,
		_obj: Box<dyn VfsNode + core::marker::Send + core::marker::Sync>,
	) -> io::Result<()> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to open a file
	fn traverse_open(
		&self,
		_components: &mut Vec<&str>,
		_option: OpenOption,
		_mode: AccessPermission,
	) -> io::Result<Arc<dyn ObjectInterface>> {
		Err(io::Error::ENOSYS)
	}

	/// Helper function to create a read-only file
	fn traverse_create_file(
		&self,
		_components: &mut Vec<&str>,
		_data: &'static [u8],
		_mode: AccessPermission,
	) -> io::Result<()> {
		Err(io::Error::ENOSYS)
	}
}

#[derive(Debug, Clone)]
struct DirectoryReader(Vec<DirectoryEntry>);

impl DirectoryReader {
	pub fn new(data: Vec<DirectoryEntry>) -> Self {
		Self(data)
	}
}

#[async_trait]
impl ObjectInterface for DirectoryReader {
	async fn readdir(&self) -> io::Result<Vec<DirectoryEntry>> {
		Ok(self.0.clone())
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
	) -> io::Result<Arc<dyn ObjectInterface>> {
		debug!("Open file {path} with {opt:?}");
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_open(&mut components, opt, mode)
	}

	/// Unlinks a file given by path
	pub fn unlink(&self, path: &str) -> io::Result<()> {
		debug!("Unlinking file {path}");
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_unlink(&mut components)
	}

	/// Remove directory given by path
	pub fn rmdir(&self, path: &str) -> io::Result<()> {
		debug!("Removing directory {path}");
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_rmdir(&mut components)
	}

	/// Create directory given by path
	pub fn mkdir(&self, path: &str, mode: AccessPermission) -> io::Result<()> {
		debug!("Create directory {path}");
		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_mkdir(&mut components, mode)
	}

	pub fn opendir(&self, path: &str) -> io::Result<Arc<dyn ObjectInterface>> {
		debug!("Open directory {path}");
		Ok(Arc::new(DirectoryReader::new(self.readdir(path)?)))
	}

	/// List given directory
	pub fn readdir(&self, path: &str) -> io::Result<Vec<DirectoryEntry>> {
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
	pub fn stat(&self, path: &str) -> io::Result<FileAttr> {
		debug!("Getting stats {path}");

		let mut components: Vec<&str> = path.split('/').collect();
		components.reverse();
		components.pop();

		self.root.traverse_stat(&mut components)
	}

	/// lstat
	pub fn lstat(&self, path: &str) -> io::Result<FileAttr> {
		debug!("Getting lstats {path}");

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
	) -> io::Result<()> {
		debug!("Mounting {path}");

		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_mount(&mut components, obj)
	}

	/// Create read-only file
	pub fn create_file(
		&self,
		path: &str,
		data: &'static [u8],
		mode: AccessPermission,
	) -> io::Result<()> {
		debug!("Create read-only file {path}");

		let mut components: Vec<&str> = path.split('/').collect();

		components.reverse();
		components.pop();

		self.root.traverse_create_file(&mut components, data, mode)
	}
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct FileAttr {
	pub st_dev: u64,
	pub st_ino: u64,
	pub st_nlink: u64,
	/// access permissions
	pub st_mode: AccessPermission,
	/// user id
	pub st_uid: u32,
	/// group id
	pub st_gid: u32,
	/// device id
	pub st_rdev: u64,
	/// size in bytes
	pub st_size: i64,
	/// block size
	pub st_blksize: i64,
	/// size in blocks
	pub st_blocks: i64,
	/// time of last access
	pub st_atim: timespec,
	/// time of last modification
	pub st_mtim: timespec,
	/// time of last status change
	pub st_ctim: timespec,
}

#[derive(TryFromPrimitive, IntoPrimitive, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u8)]
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

#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, PartialEq)]
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

pub fn create_file(name: &str, data: &'static [u8], mode: AccessPermission) -> io::Result<()> {
	FILESYSTEM
		.get()
		.ok_or(io::Error::EINVAL)?
		.create_file(name, data, mode)
}

/// Removes an empty directory.
pub fn remove_dir(path: &str) -> io::Result<()> {
	FILESYSTEM.get().ok_or(io::Error::EINVAL)?.rmdir(path)
}

pub fn unlink(path: &str) -> io::Result<()> {
	FILESYSTEM.get().ok_or(io::Error::EINVAL)?.unlink(path)
}

/// Creates a new, empty directory at the provided path
pub fn create_dir(path: &str, mode: AccessPermission) -> io::Result<()> {
	FILESYSTEM.get().ok_or(io::Error::EINVAL)?.mkdir(path, mode)
}

/// Returns an vector with all the entries within a directory.
pub fn readdir(name: &str) -> io::Result<Vec<DirectoryEntry>> {
	debug!("Read directory {name}");

	FILESYSTEM.get().ok_or(io::Error::EINVAL)?.readdir(name)
}

pub fn read_stat(name: &str) -> io::Result<FileAttr> {
	FILESYSTEM.get().ok_or(io::Error::EINVAL)?.stat(name)
}

pub fn read_lstat(name: &str) -> io::Result<FileAttr> {
	FILESYSTEM.get().ok_or(io::Error::EINVAL)?.lstat(name)
}

pub fn open(name: &str, flags: OpenOption, mode: AccessPermission) -> io::Result<FileDescriptor> {
	// mode is 0x777 (0b0111_0111_0111), when flags | O_CREAT, else 0
	// flags is bitmask of O_DEC_* defined above.
	// (taken from rust stdlib/sys hermit target )

	debug!("Open {name}, {flags:?}, {mode:?}");

	let fs = FILESYSTEM.get().ok_or(io::Error::EINVAL)?;
	if let Ok(file) = fs.open(name, flags, mode) {
		let fd = insert_object(file)?;
		Ok(fd)
	} else {
		Err(io::Error::EINVAL)
	}
}

/// Open a directory to read the directory entries
pub(crate) fn opendir(name: &str) -> io::Result<FileDescriptor> {
	let obj = FILESYSTEM.get().ok_or(io::Error::EINVAL)?.opendir(name)?;
	insert_object(obj)
}

use crate::fd::{self, FileDescriptor};

pub fn file_attributes(path: &str) -> io::Result<FileAttr> {
	FILESYSTEM.get().ok_or(io::Error::EINVAL)?.lstat(path)
}

#[allow(clippy::len_without_is_empty)]
#[derive(Debug, Copy, Clone)]
pub struct Metadata(FileAttr);

impl Metadata {
	/// Returns the size of the file, in bytes
	pub fn len(&self) -> usize {
		self.0.st_size.try_into().unwrap()
	}

	/// Returns true if this metadata is for a file.
	pub fn is_file(&self) -> bool {
		self.0.st_mode.contains(AccessPermission::S_IFREG)
	}

	/// Returns true if this metadata is for a directory.
	pub fn is_dir(&self) -> bool {
		self.0.st_mode.contains(AccessPermission::S_IFDIR)
	}

	/// Returns the last modification time listed in this metadata.
	pub fn modified(&self) -> io::Result<SystemTime> {
		Ok(SystemTime::from(self.0.st_mtim))
	}

	/// Returns the last modification time listed in this metadata.
	pub fn accessed(&self) -> io::Result<SystemTime> {
		Ok(SystemTime::from(self.0.st_atim))
	}
}

/// Given a path, query the file system to get information about a file, directory, etc.
pub fn metadata(path: &str) -> io::Result<Metadata> {
	Ok(Metadata(file_attributes(path)?))
}

#[derive(Debug)]
pub struct File {
	fd: FileDescriptor,
	path: String,
}

impl File {
	/// Creates a new file in read-write mode; error if the file exists.
	///
	/// This function will create a file if it does not exist, or return
	/// an error if it does. This way, if the call succeeds, the file
	/// returned is guaranteed to be new.
	pub fn create(path: &str) -> io::Result<Self> {
		let fd = open(
			path,
			OpenOption::O_CREAT | OpenOption::O_RDWR,
			AccessPermission::from_bits(0o666).unwrap(),
		)?;

		Ok(File {
			fd,
			path: path.to_string(),
		})
	}

	/// Attempts to open a file in read-write mode.
	pub fn open(path: &str) -> io::Result<Self> {
		let fd = open(
			path,
			OpenOption::O_RDWR,
			AccessPermission::from_bits(0o666).unwrap(),
		)?;

		Ok(File {
			fd,
			path: path.to_string(),
		})
	}

	pub fn metadata(&self) -> io::Result<Metadata> {
		metadata(&self.path)
	}
}

impl crate::io::Read for File {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		let buf = unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr().cast(), buf.len()) };
		fd::read(self.fd, buf)
	}
}

impl crate::io::Write for File {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		fd::write(self.fd, buf)
	}
}

impl Drop for File {
	fn drop(&mut self) {
		let _ = remove_object(self.fd);
	}
}
