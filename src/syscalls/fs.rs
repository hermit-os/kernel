use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use synch::spinlock::Spinlock;

/*
Design:
- want to support different backends. One of them virtiofs.
- want to support multiple mounted filesystems at once.
- for simplicity: no overlays. All 'folders' in / are mountpoints!
- manage all files in a global map. Do not hand out references, let syscalls operate by passing in closures (fd_op())

- we internally treat all file systems as posix filesystems.
- Have two traits. One representing a filesystem, another a file: PosixFileSystem and PosixFile
- filesystem.open creates new file
- trait methods like open return Result<....>, so we can catch errors on eg open() and NOT permanently assign an fd to it!

- have a FUSE filesystem, which implements both PosixFileSystem and PosixFile
- fuse can have various FuseInterface backends. These only have to provide fuse command send/receive capabilites.
- virtiofs implements FuseInterface and sends commands via virtio queues.

- fd management is only relevant for "user" facing code. We don't care how fuse etc. manages nodes internally.
- But we still want to have a list of open files and mounted filesystems (here in fs.rs).

Open Questions:
- what is the maximum number of open files I want to support? if small, could have static allocation, no need for hashmap?
- create Stdin/out virtual files, assign fd's 0-2. Instanciate them on program start. currently fd 0-2 are hardcoded exceptions.
- optimize callchain? how does LTO work here?:
	- app calls rust.open (which is stdlib hermit/fs.rs) [https://github.com/rust-lang/rust/blob/master/src/libstd/sys/hermit/fs.rs#L267]
	- abi::open() (hermit-sys crate)
	- [KERNEL BORDER] (uses C-interface. needed? Could just be alternative to native rust?)
	- hermit-lib/....rs/sys_open()
	- SyscallInterface.open (via &'static dyn ref)
	- Filesystem::open()
	- Fuse::open()
	- VirtiofsDriver::send_command(...)
	- [HYPERVISOR BORDER] (via virtio)
	- virtiofsd receives fuse command and sends reply

TODO:
- FileDescriptor newtype
*/

// TODO: lazy static could be replaced with explicit init on OS boot.
lazy_static! {
	pub static ref FILESYSTEM: Spinlock<Filesystem> = Spinlock::new(Filesystem::new());
}

pub struct Filesystem {
	// Keep track of mount-points
	mounts: BTreeMap<String, Box<dyn PosixFileSystem>>,

	// Keep track of open files
	files: BTreeMap<u64, Box<dyn PosixFile>>,
}

impl Filesystem {
	pub fn new() -> Self {
		Self {
			mounts: BTreeMap::new(),
			files: BTreeMap::new(),
		}
	}

	/// Returns next free file-descriptor. We map index in files BTreeMap as fd's.
	/// Done determining the current biggest stored index.
	/// This is efficient, since BTreeMap's iter() calculates min and max key directly.
	/// see https://github.com/rust-lang/rust/issues/62924
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
	fn add_file(&mut self, file: Box<dyn PosixFile>) -> u64 {
		let fd = self.assign_new_fd();
		self.files.insert(fd, file);
		fd
	}

	/// parses path `/MOUNTPOINT/internal-path` into mount-filesystem and internal_path
	/// Returns (PosixFileSystem, internal_path) or Error on failure.
	fn parse_path<'a, 'b>(
		&'a self,
		path: &'b str,
	) -> Result<(&'a Box<dyn PosixFileSystem>, &'b str), FileError> {
		// assert start with / (no pwd relative!), split path at /, look first element. Determine backing fs. If non existent, -ENOENT
		if !path.starts_with("/") {
			info!("Relative paths not allowed!");
			return Err(FileError::ENOENT());
		}
		let mut pathsplit = path.splitn(3, "/");
		pathsplit.next(); // always empty, since first char is /
		let mount = pathsplit.next().unwrap();
		let internal_path = pathsplit.next().unwrap(); //TODO: this can fail from userspace, eg when passing "/test"

		if let Some(fs) = self.mounts.get(mount) {
			Ok((fs, internal_path))
		} else {
			info!(
				"Trying to open file on non-existing mount point '{}'!",
				mount
			);
			return Err(FileError::ENOENT());
		}
	}

	/// Tries to open file at given path (/MOUNTPOINT/internal-path).
	/// Looks up MOUNTPOINT in mounted dirs, passes internal-path to filesystem backend
	/// Returns the file descriptor of the newly opened file, or an error on failure
	pub fn open(&mut self, path: &str, perms: FilePerms) -> Result<u64, FileError> {
		info!("Opening file {} {:?}", path, perms);
		let (fs, internal_path) = self.parse_path(path)?;
		let file = fs.open(internal_path, perms)?;
		Ok(self.add_file(file))
	}

	pub fn close(&mut self, fd: u64) {
		info!("Closing fd {}", fd);
		if let Some(file) = self.files.get_mut(&fd) {
			file.close().unwrap(); // TODO: handle error
		}
		self.files.remove(&fd);
	}

	/// Unlinks a file given by path
	pub fn unlink(&mut self, path: &str) -> Result<(), FileError> {
		info!("Unlinking file {}", path);
		let (fs, internal_path) = self.parse_path(path)?;
		fs.unlink(internal_path)?;
		Ok(())
	}

	/// Create new backing-fs at mountpoint mntpath
	pub fn mount(&mut self, mntpath: &str, mntobj: Box<dyn PosixFileSystem>) -> Result<(), ()> {
		info!("Mounting {}", mntpath);
		if mntpath.contains("/") {
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
		self.mounts.insert(mntpath.to_owned(), mntobj);
		Ok(())
	}

	/// Run closure on file referenced by file descriptor.
	pub fn fd_op(&mut self, fd: u64, f: impl FnOnce(&mut Box<dyn PosixFile>)) {
		f(self.files.get_mut(&fd).unwrap());
	}
}

#[derive(Debug)]
pub enum FileError {
	ENOENT(),
	ENOSYS(),
}

pub trait PosixFileSystem {
	fn open(&self, _path: &str, _perms: FilePerms) -> Result<Box<dyn PosixFile>, FileError>;
	fn unlink(&self, _path: &str) -> Result<(), FileError>;
}

pub trait PosixFile {
	fn close(&mut self) -> Result<(), FileError>;
	fn read(&mut self, len: u32) -> Result<Vec<u8>, FileError>;
	fn write(&mut self, buf: &[u8]) -> Result<u64, FileError>;
	fn lseek(&mut self, offset: isize, whence: SeekWhence) -> Result<usize, FileError>;
}

// TODO: raw is partially redundant, create nicer interface
#[derive(Clone, Copy, Debug, Default)]
pub struct FilePerms {
	pub write: bool,
	pub creat: bool,
	pub excl: bool,
	pub trunc: bool,
	pub append: bool,
	pub raw: u32,
	pub mode: u32,
}

pub enum SeekWhence {
	Set,
	Cur,
	End,
}
