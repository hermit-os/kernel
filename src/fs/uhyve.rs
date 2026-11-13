use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::string::String;
use alloc::sync::Arc;
use core::mem::MaybeUninit;

use async_lock::Mutex;
use embedded_io::{ErrorType, Read, Write};
use memory_addresses::VirtAddr;
use uhyve_interface::GuestPhysAddr;
use uhyve_interface::v3::Hypercall;
use uhyve_interface::v3::util::RawResult;
use uhyve_interface::v3::parameters::{
	CloseParams, GetdentParams, GetdentResult, LseekParams, LseekWhence, OpenParams, ReadParams,
	StatParams, StatTarget, UnlinkParams, WriteParams,
};

use crate::arch::mm::paging::virtual_to_physical;
use crate::env::FdtStartInfo;
use crate::errno::Errno;
use crate::fd::{Fd, RawFd};
use crate::fs::{
	self, AccessPermission, FileAttr, NodeKind, ObjectInterface, OpenOption, SeekWhence, VfsNode,
};
use crate::syscalls::DirentFormat;
use crate::uhyve::{as_mut_sized_guest_buf, as_sized_guest_buf, uhyve_hypercall};
use crate::{env, io};

fn stat_hypercall(target: StatTarget) -> io::Result<FileAttr> {
	let mut attr = FileAttr::default();
	let mut params = StatParams {
		target: target.into(),
		attr: phys_addr((&raw mut attr).cast::<FileAttr>()),
		ret: RawResult::default(),
	};
	uhyve_hypercall(Hypercall::FileStat(&mut params));
	params
		.ret
		.decode()
		.unwrap()
		.map_err(|e| Errno::try_from(e).unwrap())?;
	Ok(attr)
}

fn phys_addr<T>(ptr: *const T) -> GuestPhysAddr {
	GuestPhysAddr::new(
		virtual_to_physical(VirtAddr::from_ptr(ptr))
			.unwrap()
			.as_u64(),
	)
}

#[derive(Debug)]
struct UhyveFileHandleInner(i32);

impl UhyveFileHandleInner {
	pub fn new(fd: RawFd) -> Self {
		Self(fd)
	}

	fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		let mut params = LseekParams {
			fd: self.0,
			whence: match whence {
				SeekWhence::Set => LseekWhence::SeekSet {
					offset: offset as u64,
				},
				SeekWhence::Cur => LseekWhence::SeekCur {
					offset: offset as i64,
				},
				SeekWhence::End => LseekWhence::SeekEnd {
					offset: offset as i64,
				},
				SeekWhence::Data => LseekWhence::SeekData {
					offset: offset as u64,
				},
				SeekWhence::Hole => LseekWhence::SeekHole {
					offset: offset as u64,
				},
			}
			.into(),
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileLseek(&mut params));
		params.ret.decode().unwrap().map(|offs| offs as isize).map_err(|e| e.into())
	}
}

impl ErrorType for UhyveFileHandleInner {
	type Error = Errno;
}

impl Read for UhyveFileHandleInner {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		let mut params = ReadParams {
			fd: self.0,
			buf: as_mut_sized_guest_buf(buf),
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileRead(&mut params));
		params
			.ret
			.decode()
			.unwrap()
			.map(|i| i as usize)
			.map_err(|e| e.into())
	}
}

impl Write for UhyveFileHandleInner {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		let mut params = WriteParams {
			fd: self.0,
			buf: as_sized_guest_buf(buf),
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&mut params));
		params
			.ret
			.decode()
			.unwrap()
			.map(|i| i as usize)
			.map_err(|e| e.into())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

impl Drop for UhyveFileHandleInner {
	fn drop(&mut self) {
		let mut close_params = CloseParams {
			fd: self.0,
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileClose(&mut close_params));
		close_params
			.ret
			.decode()
			.unwrap()
			.unwrap_or_else(|e| panic!("Can't close fd {}: {e:?}", self.0));
	}
}

pub struct UhyveFileHandle(Arc<Mutex<UhyveFileHandleInner>>);

impl UhyveFileHandle {
	pub fn new(fd: RawFd) -> Self {
		Self(Arc::new(Mutex::new(UhyveFileHandleInner::new(fd))))
	}
}

impl ObjectInterface for UhyveFileHandle {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		self.0.lock().await.read(buf)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		self.0.lock().await.write(buf)
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		self.0.lock().await.lseek(offset, whence)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		stat_hypercall(StatTarget::Fd(self.0.lock().await.0))
	}
}

impl Clone for UhyveFileHandle {
	fn clone(&self) -> Self {
		Self(self.0.clone())
	}
}

pub struct UhyveDirectoryHandle {
	/// Guest fd of the directory, mapped in Uhyve's fd layer.
	fd: i32,
}

impl UhyveDirectoryHandle {
	pub const fn new(fd: i32) -> Self {
		Self { fd }
	}
}

impl Drop for UhyveDirectoryHandle {
	fn drop(&mut self) {
		let mut close_params = CloseParams {
			fd: self.fd,
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileClose(&mut close_params));
	}
}

impl ObjectInterface for UhyveDirectoryHandle {
	async fn getdents(
		&self,
		buf: &mut [MaybeUninit<u8>],
		format: DirentFormat,
	) -> io::Result<usize> {
		match format {
			DirentFormat::PosixDent => unimplemented!("Uhyve has no POSIX dirent support (yet)"),
			DirentFormat::Dirent64 => {
				let mut read_dir_params = GetdentParams {
					fd: self.fd,
					buf: as_mut_sized_guest_buf(buf),
					ret: RawResult::default(),
				};
				uhyve_hypercall(Hypercall::Getdents(&mut read_dir_params));
				match read_dir_params.ret.decode().unwrap() {
					GetdentResult::Success(len) => Ok(len.get().try_into().unwrap()),
					GetdentResult::EndOfDirectory => Ok(0),
					GetdentResult::Errno(errno) => Err(errno.into()),
				}
			}
		}
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		stat_hypercall(StatTarget::Fd(self.fd))
	}
}

#[derive(Debug)]
pub(crate) struct UhyveDirectory {
	/// The external path of this directory.
	///
	/// Before talking to virtio-fs, the relative path inside this directory is
	/// adjoined with this prefix.
	prefix: String,
}

impl UhyveDirectory {
	pub const fn new(prefix: String) -> Self {
		UhyveDirectory { prefix }
	}

	fn traversal_path(&self, path: &str) -> CString {
		let prefix = self.prefix.as_str();
		let prefix = prefix.strip_suffix("/").unwrap_or(prefix);
		if path.is_empty() {
			return CString::new(prefix).unwrap();
		}
		let path = [prefix, path].join("/");
		CString::new(path).unwrap()
	}
}

impl VfsNode for UhyveDirectory {
	/// Returns the node type
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		let path = self.traversal_path("");
		stat_hypercall(StatTarget::Path(phys_addr(path.as_ptr())))
	}

	fn traverse_stat(&self, path: &str) -> io::Result<FileAttr> {
		let path = self.traversal_path(path);
		stat_hypercall(StatTarget::Path(phys_addr(path.as_ptr())))
	}

	fn traverse_lstat(&self, path: &str) -> io::Result<FileAttr> {
		let path = self.traversal_path(path);
		stat_hypercall(StatTarget::Link(phys_addr(path.as_ptr())))
	}

	fn traverse_open(
		&self,
		path: &str,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<async_lock::RwLock<Fd>>> {
		let path = self.traversal_path(path);

		let mut open_params = OpenParams {
			path: GuestPhysAddr::new(
				virtual_to_physical(VirtAddr::from_ptr(path.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			flags: opt.bits(),
			mode: mode.bits(),
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileOpen(&mut open_params));
		match open_params.ret.decode().unwrap() {
			// Assumption: Uhyve will never return a standard stream.
			Ok(fd) => {
				let obj = if opt.contains(OpenOption::O_DIRECTORY) {
					UhyveDirectoryHandle::new(fd).into()
				} else {
					UhyveFileHandle::new(fd).into()
				};
				Ok(Arc::new(async_lock::RwLock::new(obj)))
			}
			Err(errno) => Err(errno.into()),
		}
	}

	fn traverse_unlink(&self, path: &str) -> io::Result<()> {
		let path = self.traversal_path(path);

		let mut unlink_params = UnlinkParams {
			path: GuestPhysAddr::new(
				virtual_to_physical(VirtAddr::from_ptr(path.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileUnlink(&mut unlink_params));
		unlink_params.ret.decode().unwrap().map_err(|e| e.into())
	}

	fn traverse_rmdir(&self, _path: &str) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	fn traverse_mkdir(&self, _path: &str, _mode: AccessPermission) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Determines the syscall interface
	fn get_object(&self) -> io::Result<Arc<async_lock::RwLock<Fd>>> {
		let path = CString::new(self.prefix.clone()).unwrap();
		let mut open_params = OpenParams {
			path: GuestPhysAddr::new(
				virtual_to_physical(VirtAddr::from_ptr(path.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			flags: (OpenOption::O_RDONLY | OpenOption::O_DIRECTORY).bits(),
			mode: 0,
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileOpen(&mut open_params));
		open_params
			.ret
			.decode()
			.unwrap()
			.map(|fd| {
				Arc::new(async_lock::RwLock::new(
					UhyveDirectoryHandle::new(fd).into(),
				))
			})
			.map_err(|e| e.into())
	}
}

pub(crate) fn init() {
	info!("Try to initialize uhyve filesystem");

	let mount_str = env::start_info().fdt().and_then(|fdt| {
		fdt.find_node("/uhyve,mounts")
			.and_then(|node| node.property("mounts"))
			.and_then(|property| property.as_str())
	});

	let Some(mount_str) = mount_str else {
		// No FDT -> Uhyve legacy mounting (to /root)
		let mount_point = hermit_var_or!("UHYVE_MOUNT", "/root").to_owned();
		info!("Mounting uhyve filesystem at {mount_point}");
		fs::FILESYSTEM
			.get()
			.unwrap()
			.mount(
				&mount_point,
				Box::new(UhyveDirectory::new(mount_point.clone())),
			)
			.expect("Mount failed. Duplicate mount_point?");
		return;
	};

	assert_ne!(mount_str.len(), 0, "Invalid /uhyve,mounts node in FDT");
	for mount_point in mount_str.split('\0') {
		info!("Mounting uhyve filesystem at {mount_point}");

		let obj = Box::new(UhyveDirectory::new(mount_point.to_owned()));
		let Err(errno) = fs::FILESYSTEM.get().unwrap().mount(mount_point, obj) else {
			continue;
		};

		assert_eq!(errno, Errno::Badf);
		debug!("Mounting of {mount_point} failed with {errno:?}. Creating missing parent folders");
		let (parent_path, _file_name) = mount_point.rsplit_once('/').unwrap();
		create_dir_recursive(parent_path, AccessPermission::S_IRWXU).unwrap();

		let obj = Box::new(UhyveDirectory::new(mount_point.to_owned()));
		fs::FILESYSTEM
			.get()
			.unwrap()
			.mount(mount_point, obj)
			.unwrap();
	}
}

/// Creates a directory and creates all missing parent directories as well.
fn create_dir_recursive(path: &str, mode: AccessPermission) -> io::Result<()> {
	trace!("create_dir_recursive: {path}");
	fs::create_dir(path, mode).or_else(|errno| {
		if errno != Errno::Badf {
			return Err(errno);
		}
		let (parent_path, _file_name) = path.rsplit_once('/').unwrap();
		create_dir_recursive(parent_path, mode)?;
		fs::create_dir(path, mode)
	})
}
