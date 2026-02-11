mod mem;
mod uhyve;
#[cfg(feature = "virtio-fs")]
pub(crate) mod virtio_fs;

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::ops::BitAnd;
use core::{fmt, slice};

use async_trait::async_trait;
use embedded_io::{Read, Write};
use hermit_sync::{InterruptSpinMutex, OnceCell};
use mem::MemDirectory;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::errno::Errno;
use crate::executor::block_on;
use crate::fd::{AccessPermission, ObjectInterface, OpenOption, insert_object, remove_object};
use crate::io;
use crate::time::{SystemTime, timespec};

static FILESYSTEM: OnceCell<Filesystem> = OnceCell::new();

static WORKING_DIRECTORY: InterruptSpinMutex<Option<String>> = InterruptSpinMutex::new(None);

static UMASK: InterruptSpinMutex<AccessPermission> =
	InterruptSpinMutex::new(AccessPermission::from_bits_retain(0o777));

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
pub(crate) trait VfsNode: Send + Sync + fmt::Debug {
	/// Determines the current node type
	fn get_kind(&self) -> NodeKind;

	/// Determines the current file attribute
	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		Err(Errno::Nosys)
	}

	/// Determines the syscall interface
	fn get_object(&self) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
		Err(Errno::Nosys)
	}

	/// Creates a new directory node
	fn traverse_mkdir(
		&self,
		_components: &mut Vec<&str>,
		_mode: AccessPermission,
	) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Deletes a directory node
	fn traverse_rmdir(&self, _components: &mut Vec<&str>) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Removes the specified file
	fn traverse_unlink(&self, _components: &mut Vec<&str>) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Opens a directory
	fn traverse_readdir(&self, _components: &mut Vec<&str>) -> io::Result<Vec<DirectoryEntry>> {
		Err(Errno::Nosys)
	}

	/// Gets file status
	fn traverse_lstat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(Errno::Nosys)
	}

	/// Gets file status
	fn traverse_stat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(Errno::Nosys)
	}

	/// Mounts a file system
	fn traverse_mount(
		&self,
		_components: &mut Vec<&str>,
		_obj: Box<dyn VfsNode>,
	) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Opens a file
	fn traverse_open(
		&self,
		_components: &mut Vec<&str>,
		_option: OpenOption,
		_mode: AccessPermission,
	) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
		Err(Errno::Nosys)
	}

	/// Creates a read-only file
	fn traverse_create_file(
		&self,
		_components: &mut Vec<&str>,
		_data: &'static [u8],
		_mode: AccessPermission,
	) -> io::Result<()> {
		Err(Errno::Nosys)
	}
}

#[derive(Clone)]
struct DirectoryReader(Vec<DirectoryEntry>);

impl DirectoryReader {
	pub fn new(data: Vec<DirectoryEntry>) -> Self {
		Self(data)
	}
}

#[async_trait]
impl ObjectInterface for DirectoryReader {
	async fn getdents(&self, _buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		let _ = &self.0; // Dummy statement to avoid warning for the moment
		unimplemented!()
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
	) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
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

	pub fn opendir(&self, path: &str) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
		debug!("Open directory {path}");
		Ok(Arc::new(async_lock::RwLock::new(DirectoryReader::new(
			self.readdir(path)?,
		))))
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
	pub fn mount(&self, path: &str, obj: Box<dyn VfsNode>) -> io::Result<()> {
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
	/// Access permissions
	pub st_mode: AccessPermission,
	/// User id
	pub st_uid: u32,
	/// Group id
	pub st_gid: u32,
	/// Device id
	pub st_rdev: u64,
	/// Size in bytes
	pub st_size: i64,
	/// Block size
	pub st_blksize: i64,
	/// Size in blocks
	pub st_blocks: i64,
	/// Time of last access
	pub st_atim: timespec,
	/// Time of last modification
	pub st_mtim: timespec,
	/// Time of last status change
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

#[derive(TryFromPrimitive, IntoPrimitive, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u8)]
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

	let mut cwd = WORKING_DIRECTORY.lock();
	*cwd = Some("/tmp".to_owned());
	drop(cwd);

	#[cfg(feature = "virtio-fs")]
	virtio_fs::init();
	if crate::env::is_uhyve() {
		uhyve::init();
	}
}

pub fn create_file(name: &str, data: &'static [u8], mode: AccessPermission) -> io::Result<()> {
	with_relative_filename(name, |name| {
		FILESYSTEM
			.get()
			.ok_or(Errno::Inval)?
			.create_file(name, data, mode)
	})
}

/// Removes an empty directory.
pub fn remove_dir(path: &str) -> io::Result<()> {
	with_relative_filename(path, |path| {
		FILESYSTEM.get().ok_or(Errno::Inval)?.rmdir(path)
	})
}

pub fn unlink(path: &str) -> io::Result<()> {
	with_relative_filename(path, |path| {
		FILESYSTEM.get().ok_or(Errno::Inval)?.unlink(path)
	})
}

/// Creates a new, empty directory at the provided path
pub fn create_dir(path: &str, mode: AccessPermission) -> io::Result<()> {
	let mask = *UMASK.lock();

	with_relative_filename(path, |path| {
		FILESYSTEM
			.get()
			.ok_or(Errno::Inval)?
			.mkdir(path, mode.bitand(mask))
	})
}

/// Creates a directory and creates all missing parent directories as well.
fn create_dir_recursive(path: &str, mode: AccessPermission) -> io::Result<()> {
	trace!("create_dir_recursive: {path}");
	create_dir(path, mode).or_else(|errno| {
		if errno != Errno::Badf {
			return Err(errno);
		}
		let (parent_path, _file_name) = path.rsplit_once('/').unwrap();
		create_dir_recursive(parent_path, mode)?;
		create_dir(path, mode)
	})
}

/// Returns an vector with all the entries within a directory.
pub fn readdir(name: &str) -> io::Result<Vec<DirectoryEntry>> {
	debug!("Read directory {name}");

	with_relative_filename(name, |name| {
		FILESYSTEM.get().ok_or(Errno::Inval)?.readdir(name)
	})
}

pub fn read_stat(name: &str) -> io::Result<FileAttr> {
	with_relative_filename(name, |name| {
		FILESYSTEM.get().ok_or(Errno::Inval)?.stat(name)
	})
}

pub fn read_lstat(name: &str) -> io::Result<FileAttr> {
	with_relative_filename(name, |name| {
		FILESYSTEM.get().ok_or(Errno::Inval)?.lstat(name)
	})
}

fn with_relative_filename<F, T>(name: &str, callback: F) -> io::Result<T>
where
	F: FnOnce(&str) -> io::Result<T>,
{
	if name.starts_with("/") {
		return callback(name);
	}

	let cwd = WORKING_DIRECTORY.lock();

	let Some(cwd) = cwd.as_ref() else {
		// Relative path with no CWD, this is weird/impossible
		return Err(Errno::Badf);
	};

	let mut path = String::with_capacity(cwd.len() + name.len() + 1);
	path.push_str(cwd);
	path.push('/');
	path.push_str(name);

	callback(&path)
}

pub fn truncate(name: &str, size: usize) -> io::Result<()> {
	with_relative_filename(name, |name| {
		let fs = FILESYSTEM.get().ok_or(Errno::Inval)?;
		let file = fs
			.open(name, OpenOption::O_TRUNC, AccessPermission::empty())
			.map_err(|_| Errno::Badf)?;

		block_on(async { file.read().await.truncate(size).await }, None)
	})
}

pub fn open(name: &str, flags: OpenOption, mode: AccessPermission) -> io::Result<RawFd> {
	// mode is 0x777 (0b0111_0111_0111), when flags | O_CREAT, else 0
	// flags is bitmask of O_DEC_* defined above.
	// (taken from rust stdlib/sys hermit target )
	let mask = *UMASK.lock();

	with_relative_filename(name, |name| {
		debug!("Open {name}, {flags:?}, {mode:?}");

		let fs = FILESYSTEM.get().ok_or(Errno::Inval)?;
		let file = fs.open(name, flags, mode.bitand(mask))?;
		let fd = insert_object(file)?;
		Ok(fd)
	})
}

pub fn get_cwd() -> io::Result<String> {
	let cwd = WORKING_DIRECTORY.lock();
	let cwd = cwd.as_ref().ok_or(Errno::Noent)?;
	Ok(cwd.clone())
}

pub fn set_cwd(cwd: &str) -> io::Result<()> {
	// TODO: check that the directory exists and that permission flags are correct

	let mut working_dir = WORKING_DIRECTORY.lock();
	if cwd.starts_with("/") {
		*working_dir = Some(cwd.to_owned());
	} else {
		let working_dir = working_dir.as_mut().ok_or(Errno::Badf)?;
		working_dir.push('/');
		working_dir.push_str(cwd);
	}

	Ok(())
}

pub fn umask(new_mask: AccessPermission) -> AccessPermission {
	let mut lock = UMASK.lock();
	let old = *lock;
	*lock = new_mask;
	old
}

/// Open a directory to read the directory entries
pub(crate) fn opendir(name: &str) -> io::Result<RawFd> {
	let obj = FILESYSTEM.get().ok_or(Errno::Inval)?.opendir(name)?;
	insert_object(obj)
}

use crate::fd::{self, RawFd};

pub fn file_attributes(path: &str) -> io::Result<FileAttr> {
	FILESYSTEM.get().ok_or(Errno::Inval)?.lstat(path)
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
	fd: RawFd,
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
			path: path.to_owned(),
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
			path: path.to_owned(),
		})
	}

	pub fn metadata(&self) -> io::Result<Metadata> {
		metadata(&self.path)
	}
}

impl embedded_io::ErrorType for File {
	type Error = crate::errno::Errno;
}

impl Read for File {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		let buf = unsafe { slice::from_raw_parts_mut(buf.as_mut_ptr().cast(), buf.len()) };
		fd::read(self.fd, buf)
	}
}

impl Write for File {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		fd::write(self.fd, buf)
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

impl Drop for File {
	fn drop(&mut self) {
		let _ = remove_object(self.fd);
	}
}
