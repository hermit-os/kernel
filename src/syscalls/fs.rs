use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Deref;

use hermit_sync::TicketMutex;

/// Design:
/// - want to support different backends. One of them virtiofs.
/// - want to support multiple mounted filesystems at once.
/// - for simplicity: no overlays. All 'folders' in / are mountpoints!
/// - manage all files in a global map. Do not hand out references, let syscalls operate by passing in closures (fd_op())
///
/// - we internally treat all file systems as posix filesystems.
/// - Have two traits. One representing a filesystem, another a file: PosixFileSystem and PosixFile
/// - filesystem.open creates new file
/// - trait methods like open return Result<....>, so we can catch errors on eg open() and NOT permanently assign an fd to it!
///
/// - have a FUSE filesystem, which implements both PosixFileSystem and PosixFile
/// - fuse can have various FuseInterface backends. These only have to provide fuse command send/receive capabilities.
/// - virtiofs implements FuseInterface and sends commands via virtio queues.
///
/// - fd management is only relevant for "user" facing code. We don't care how fuse etc. manages nodes internally.
/// - But we still want to have a list of open files and mounted filesystems (here in fs.rs).
///
/// Open Questions:
/// - what is the maximum number of open files I want to support? if small, could have static allocation, no need for hashmap?
/// - create Stdin/out virtual files, assign fd's 0-2. Instantiate them on program start. currently fd 0-2 are hardcoded exceptions.
/// - optimize callchain? how does LTO work here?:
///     - app calls rust.open (which is stdlib hermit/fs.rs) [https://github.com/rust-lang/rust/blob/master/src/libstd/sys/hermit/fs.rs#L267]
///     - abi::open() (hermit-sys crate)
///     - [KERNEL BORDER] (uses C-interface. needed? Could just be alternative to native rust?)
///     - hermit-lib/....rs/sys_open()
///     - SyscallInterface.open (via &'static dyn ref)
///     - Filesystem::open()
///     - Fuse::open()
///     - VirtiofsDriver::send_command(...)
///     - [HYPERVISOR BORDER] (via virtio)
///     - virtiofsd receives fuse command and sends reply
///
/// TODO:
/// - FileDescriptor newtype
use crate::env::is_uhyve;
use crate::errno;
pub use crate::fs::fuse::fuse_dirent as Dirent;

// TODO: lazy static could be replaced with explicit init on OS boot.
pub static FILESYSTEM: TicketMutex<Filesystem> = TicketMutex::new(Filesystem::new());

pub struct Filesystem {
	// Keep track of mount-points
	mounts: BTreeMap<String, Box<dyn PosixFileSystem + Send>>,

	// Keep track of open files
	files: BTreeMap<u64, Box<dyn PosixFile + Send>>,
}

impl Filesystem {
	pub const fn new() -> Self {
		Self {
			mounts: BTreeMap::new(),
			files: BTreeMap::new(),
		}
	}

	/// Returns next free file-descriptor. We map index in files BTreeMap as fd's.
	/// Done determining the current biggest stored index.
	/// This is efficient, since BTreeMap's iter() calculates min and max key directly.
	/// see <https://github.com/rust-lang/rust/issues/62924>
	fn assign_new_fd(&self) -> u64 {
		// BTreeMap has efficient max/min index calculation. One way to access these is the following iter.
		// Add 1 to get next never-assigned fd num
		if let Some((fd, _)) = self.files.iter().next_back() {
			fd + 1
		} else {
			3 // start at 3, to reserve stdin/out/err
		}
	}

	/// Gets a new fd for a file and inserts it into open files.
	/// Returns file descriptor
	fn add_file(&mut self, file: Box<dyn PosixFile + Send>) -> u64 {
		let fd = self.assign_new_fd();
		self.files.insert(fd, file);
		fd
	}

	/// parses path `/MOUNTPOINT/internal-path` into mount-filesystem and internal_path
	/// Returns (PosixFileSystem, internal_path) or Error on failure.
	fn parse_path<'a, 'b>(
		&'a self,
		path: &'b str,
	) -> Result<(&'a (dyn PosixFileSystem + Send), &'b str), FileError> {
		let mut pathsplit = path.splitn(3, '/');

		if path.starts_with('/') {
			pathsplit.next(); // empty, since first char is /

			let mount = pathsplit.next().unwrap();
			let internal_path = pathsplit.next().unwrap_or("/");
			if let Some(fs) = self.mounts.get(mount) {
				return Ok((fs.deref(), internal_path));
			}

			warn!(
				"Trying to open file on non-existing mount point '{}'!",
				mount
			);
		} else {
			let mount = if !is_uhyve() {
				option_env!("HERMIT_WD").unwrap_or("root")
			} else {
				"."
			};
			let internal_path = pathsplit.next().unwrap_or("/");

			debug!(
				"Assume that the directory '{}' is used as mount point!",
				mount
			);

			if let Some(fs) = self.mounts.get(mount) {
				return Ok((fs.deref(), internal_path));
			}

			warn!(
				"Trying to open file on non-existing mount point '{}'!",
				mount
			);
		}

		Err(FileError::ENOENT)
	}

	/// Tries to open file at given path (/MOUNTPOINT/internal-path).
	/// Looks up MOUNTPOINT in mounted dirs, passes internal-path to filesystem backend
	/// Returns the file descriptor of the newly opened file, or an error on failure
	pub fn open(&mut self, path: &str, perms: FilePerms) -> Result<u64, FileError> {
		debug!("Opening file {} {:?}", path, perms);
		let (fs, internal_path) = self.parse_path(path)?;
		let file = fs.open(internal_path, perms)?;
		Ok(self.add_file(file))
	}

	/// Similar to open
	pub fn opendir(&mut self, path: &str) -> Result<u64, FileError> {
		debug!("Opening dir {}", path);
		let (fs, internal_path) = self.parse_path(path)?;
		let file = fs.opendir(internal_path)?;
		Ok(self.add_file(file))
	}

	/// Closes a file with given fd.
	/// If the file is currently open, closes it
	/// Remove the file from map of open files
	pub fn close(&mut self, fd: u64) {
		debug!("Closing fd {}", fd);
		if let Some(file) = self.files.get_mut(&fd) {
			file.close().unwrap(); // TODO: handle error
		}
		self.files.remove(&fd);
	}

	/// Unlinks a file given by path
	pub fn unlink(&mut self, path: &str) -> Result<(), FileError> {
		debug!("Unlinking file {}", path);
		let (fs, internal_path) = self.parse_path(path)?;
		fs.unlink(internal_path)?;
		Ok(())
	}

	/// Remove directory given by path
	pub fn rmdir(&mut self, path: &str) -> Result<(), FileError> {
		debug!("Removing directory {}", path);
		let (fs, internal_path) = self.parse_path(path)?;
		fs.rmdir(internal_path)?;
		Ok(())
	}

	/// Create directory given by path
	pub fn mkdir(&mut self, path: &str, mode: u32) -> Result<(), FileError> {
		debug!("Removing directory {}", path);
		let (fs, internal_path) = self.parse_path(path)?;
		fs.mkdir(internal_path, mode)?;
		Ok(())
	}

	/// stat
	pub fn stat(&mut self, path: &str, stat: *mut FileAttr) -> Result<(), FileError> {
		debug!("Getting stats {}", path);
		let (fs, internal_path) = self.parse_path(path)?;
		fs.stat(internal_path, stat)?;
		Ok(())
	}

	/// lstat
	pub fn lstat(&mut self, path: &str, stat: *mut FileAttr) -> Result<(), FileError> {
		debug!("Getting lstats {}", path);
		let (fs, internal_path) = self.parse_path(path)?;
		fs.lstat(internal_path, stat)?;
		Ok(())
	}

	/// Create new backing-fs at mountpoint mntpath
	#[cfg(feature = "fs")]
	pub fn mount(
		&mut self,
		mntpath: &str,
		mntobj: Box<dyn PosixFileSystem + Send>,
	) -> Result<(), ()> {
		use alloc::string::ToString;

		debug!("Mounting {}", mntpath);
		if mntpath.contains('/') {
			warn!(
				"Trying to mount at '{}', but slashes in name are not supported!",
				mntpath
			);
			return Err(());
		}

		// if mounts contains path already abort
		if self.mounts.contains_key(mntpath) {
			warn!("Mountpoint already exists!");
			return Err(());
		}

		// insert filesystem into mounts, done
		self.mounts.insert(mntpath.to_string(), mntobj);

		Ok(())
	}

	/// Run closure on file referenced by file descriptor.
	pub fn fd_op(&mut self, fd: u64, f: impl FnOnce(&mut Box<dyn PosixFile + Send>)) {
		f(self.files.get_mut(&fd).unwrap());
	}
}

// TODO: Integrate with src/errno.rs ?
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, FromPrimitive, ToPrimitive)]
pub enum FileError {
	ENOENT = errno::ENOENT as isize,
	#[cfg(any(feature = "fs", feature = "pci"))]
	ENOSYS = errno::ENOSYS as isize,
	#[cfg(any(feature = "fs", feature = "pci"))]
	EIO = errno::EIO as isize,
	#[cfg(feature = "pci")]
	EBADF = errno::EBADF as isize,
	#[cfg(feature = "pci")]
	EISDIR = errno::EISDIR as isize,
}

pub trait PosixFileSystem {
	fn open(&self, _path: &str, _perms: FilePerms) -> Result<Box<dyn PosixFile + Send>, FileError>;
	fn opendir(&self, path: &str) -> Result<Box<dyn PosixFile + Send>, FileError>;
	fn unlink(&self, _path: &str) -> Result<(), FileError>;

	fn rmdir(&self, _path: &str) -> Result<(), FileError>;
	fn mkdir(&self, name: &str, mode: u32) -> Result<i32, FileError>;
	fn stat(&self, _path: &str, stat: *mut FileAttr) -> Result<(), FileError>;
	fn lstat(&self, _path: &str, stat: *mut FileAttr) -> Result<(), FileError>;
}

pub trait PosixFile {
	fn close(&mut self) -> Result<(), FileError>;
	fn read(&mut self, len: u32) -> Result<Vec<u8>, FileError>;
	fn write(&mut self, buf: &[u8]) -> Result<u64, FileError>;
	fn lseek(&mut self, offset: isize, whence: SeekWhence) -> Result<usize, FileError>;

	fn readdir(&mut self) -> Result<*const Dirent, FileError>;
	fn fstat(&self, stat: *mut FileAttr) -> Result<(), FileError>;
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
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
pub enum PosixFileType {
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

// TODO: raw is partially redundant, create nicer interface
#[derive(Clone, Copy, Debug, Default)]
pub struct FilePerms {
	pub write: bool,
	pub creat: bool,
	pub excl: bool,
	pub trunc: bool,
	pub append: bool,
	pub directio: bool,
	pub raw: u32,
	pub mode: u32,
}

#[derive(Debug, FromPrimitive, ToPrimitive)]
pub enum SeekWhence {
	Set = 0,
	Cur = 1,
	End = 2,
	Data = 3,
	Hole = 4,
}
