use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::string::String;
use alloc::sync::Arc;

use async_lock::Mutex;
use async_trait::async_trait;
use embedded_io::{ErrorType, Read, Write};
use memory_addresses::VirtAddr;
use uhyve_interface::parameters::{
	CloseParams, LseekParams, OpenParams, ReadParams, UnlinkParams, WriteParams,
};
use uhyve_interface::{GuestPhysAddr, GuestVirtAddr, Hypercall};

use crate::arch::mm::paging;
use crate::env::fdt;
use crate::errno::Errno;
use crate::fs::{
	self, AccessPermission, FileAttr, NodeKind, ObjectInterface, OpenOption, SeekWhence, VfsNode,
	create_dir_recursive,
};
use crate::io;
use crate::syscalls::interfaces::uhyve::uhyve_hypercall;

#[derive(Debug)]
struct UhyveFileHandleInner(i32);

impl UhyveFileHandleInner {
	pub fn new(fd: i32) -> Self {
		Self(fd)
	}

	fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		let mut lseek_params = LseekParams {
			fd: self.0,
			offset,
			whence: u8::from(whence).into(),
		};
		uhyve_hypercall(Hypercall::FileLseek(&mut lseek_params));

		if lseek_params.offset >= 0 {
			Ok(lseek_params.offset)
		} else {
			Err(Errno::Inval)
		}
	}
}

impl ErrorType for UhyveFileHandleInner {
	type Error = Errno;
}

impl Read for UhyveFileHandleInner {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		let mut read_params = ReadParams {
			fd: self.0,
			buf: GuestVirtAddr::from_ptr(buf.as_mut_ptr()),
			len: buf.len(),
			ret: 0,
		};
		uhyve_hypercall(Hypercall::FileRead(&mut read_params));

		if read_params.ret >= 0 {
			Ok(read_params.ret.try_into().unwrap())
		} else {
			Err(Errno::Io)
		}
	}
}

impl Write for UhyveFileHandleInner {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		let write_params = WriteParams {
			fd: self.0,
			buf: GuestVirtAddr::from_ptr(buf.as_ptr()),
			len: buf.len(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&write_params));

		Ok(write_params.len)
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

impl Drop for UhyveFileHandleInner {
	fn drop(&mut self) {
		let mut close_params = CloseParams { fd: self.0, ret: 0 };
		uhyve_hypercall(Hypercall::FileClose(&mut close_params));
		if close_params.ret != 0 {
			let ret = close_params.ret; // circumvent packed field access
			panic!("Can't close fd {} - return value {ret}", self.0);
		}
	}
}

struct UhyveFileHandle(Arc<Mutex<UhyveFileHandleInner>>);

impl UhyveFileHandle {
	pub fn new(fd: i32) -> Self {
		Self(Arc::new(Mutex::new(UhyveFileHandleInner::new(fd))))
	}
}

#[async_trait]
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
}

impl Clone for UhyveFileHandle {
	fn clone(&self) -> Self {
		Self(self.0.clone())
	}
}

#[derive(Clone, Debug)]
// Unlike src/fs/fuse.rs, we do not prepend / before $prefix
// TODO: rename to `UhyveEntry`
pub(crate) struct UhyveDirectory {
	prefix: String,
}

impl UhyveDirectory {
	pub fn new(prefix: Option<String>) -> Self {
		UhyveDirectory {
			prefix: prefix.unwrap_or_default(),
		}
	}

	fn traversal_path(&self, component: &str) -> CString {
		let mut path = self.prefix.clone();
		if !self.prefix.is_empty() {
			path.push('/');
		}
		path.push_str(component);
		CString::new(path).unwrap()
	}
}

#[async_trait]
impl VfsNode for UhyveDirectory {
	/// Returns the node type
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn dup(&self) -> Box<dyn VfsNode> {
		Box::new(self.clone())
	}

	async fn traverse_once(&self, component: &str) -> io::Result<Box<dyn VfsNode>> {
		let mut prefix = self.prefix.clone();
		if !prefix.is_empty() {
			prefix.push('/');
		}
		prefix.push_str(component);
		Ok(Box::new(Self { prefix }))
	}

	async fn traverse_multiple(&self, mut path: &str) -> io::Result<Box<dyn VfsNode>> {
		let mut prefix = self.prefix.clone();
		// this part prevents inserting double-slashes or no slashes between prefix and path
		if !path.is_empty() {
			if let Some(x) = path.strip_prefix("/") {
				path = x;
			} else {
				return Err(Errno::Nosys);
			}
			if !prefix.is_empty() {
				prefix.push('/');
			}
		}
		prefix.push_str(path);
		Ok(Box::new(Self { prefix }))
	}

	async fn stat(&self) -> io::Result<FileAttr> {
		Err(Errno::Nosys)
	}

	async fn lstat(&self) -> io::Result<FileAttr> {
		Err(Errno::Nosys)
	}

	async fn open(
		&self,
		component: &str,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
		let path = self.traversal_path(component);

		let mut open_params = OpenParams {
			name: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(path.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			flags: opt.bits(),
			mode: mode.bits() as i32,
			ret: -1,
		};
		uhyve_hypercall(Hypercall::FileOpen(&mut open_params));

		if open_params.ret > 0 {
			Ok(Arc::new(async_lock::RwLock::new(UhyveFileHandle::new(
				open_params.ret,
			))))
		} else {
			Err(Errno::Io)
		}
	}

	async fn unlink(&self, component: &str) -> io::Result<()> {
		let path = self.traversal_path(component);

		let mut unlink_params = UnlinkParams {
			name: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(path.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			ret: -1,
		};
		uhyve_hypercall(Hypercall::FileUnlink(&mut unlink_params));

		if unlink_params.ret == 0 {
			Ok(())
		} else {
			Err(Errno::Io)
		}
	}

	async fn rmdir(&self, _component: &str) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	async fn mkdir(&self, _component: &str, _mode: AccessPermission) -> io::Result<()> {
		Err(Errno::Nosys)
	}
}

pub(crate) fn init() {
	info!("Try to initialize uhyve filesystem");
	let mount_str = fdt().and_then(|fdt| {
		fdt.find_node("/uhyve,mounts")
			.and_then(|node| node.property("mounts"))
			.and_then(|property| property.as_str())
	});
	if let Some(mount_str) = mount_str {
		assert_ne!(mount_str.len(), 0, "Invalid /uhyve,mounts node in FDT");
		for mount_point in mount_str.split('\0') {
			info!("Mounting uhyve filesystem at {mount_point}");

			if let Err(errno) = fs::FILESYSTEM.get().unwrap().mount(
				mount_point,
				Box::new(UhyveDirectory::new(Some(mount_point.to_owned()))),
			) {
				assert_eq!(errno, Errno::Badf);
				debug!(
					"Mounting of {mount_point} failed with {errno:?}. Creating missing parent folders"
				);
				let (parent_path, _file_name) = mount_point.rsplit_once('/').unwrap();
				create_dir_recursive(parent_path, AccessPermission::S_IRWXU).unwrap();

				fs::FILESYSTEM
					.get()
					.unwrap()
					.mount(
						mount_point,
						Box::new(UhyveDirectory::new(Some(mount_point.to_owned()))),
					)
					.unwrap();
			}
		}
	} else {
		// No FDT -> Uhyve legacy mounting (to /root)
		let mount_point = hermit_var_or!("UHYVE_MOUNT", "/root").to_owned();
		info!("Mounting uhyve filesystem at {mount_point}");
		fs::FILESYSTEM
			.get()
			.unwrap()
			.mount(
				&mount_point,
				Box::new(UhyveDirectory::new(Some(mount_point.clone()))),
			)
			.expect("Mount failed. Duplicate mount_point?");
	}
}
