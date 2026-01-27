use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::ffi::CString;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use async_lock::Mutex;
use async_trait::async_trait;
use embedded_io::{ErrorType, Read, Write};
use memory_addresses::VirtAddr;
use uhyve_interface::GuestPhysAddr;
use uhyve_interface::v2::Hypercall;
use uhyve_interface::v2::parameters::{
	CloseParams, LseekParams, OpenParams, ReadParams, UnlinkParams, WriteParams,
};

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
			offset: offset.try_into().unwrap(),
			whence: u8::from(whence).into(),
		};
		uhyve_hypercall(Hypercall::FileLseek(&mut lseek_params));
		// TODO: Although we can generally assume that what Uhyve delivers should be
		// correct for now, it might make sense to build in checks (or at least debug_assert's)
		match lseek_params.offset {
			offset if offset >= 0 => Ok(offset.try_into().unwrap()),
			errno if errno < 0 => Err((errno as i32).abs().try_into().unwrap()),
			_ => {
				debug!("Uhyve lseek hypercall yielded a zero.");
				Err(Errno::Inval)
			}
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
			buf: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(buf.as_mut_ptr()))
					.unwrap()
					.as_u64(),
			),
			len: buf.len().try_into().unwrap(),
			ret: 0i64,
		};
		uhyve_hypercall(Hypercall::FileRead(&mut read_params));
		match read_params.ret {
			ret if ret >= 0 => Ok(ret.try_into().unwrap()),
			_ => Err((read_params.ret as i32).abs().try_into().unwrap()),
		}
	}
}

impl Write for UhyveFileHandleInner {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		let mut write_params = WriteParams {
			fd: self.0,
			buf: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(buf.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			len: buf.len().try_into().unwrap(),
			ret: 0i64,
		};
		// fd refers to a regular file
		uhyve_hypercall(Hypercall::FileWrite(&mut write_params));
		match write_params.ret {
			// Assumption: fd is a regular file, a zero is only valid if the len
			// (aka. "count") is also zero. Otherwise, however, we assume that something
			// is wrong in Hermit<>Uhyve communication.
			ret if ret > 0 || (ret == 0 && write_params.len == 0) => Ok(ret.try_into().unwrap()),
			errno if errno < 0 => Err((errno as i32).abs().try_into().unwrap()),
			_ => {
				debug!("Uhyve write hypercall yielded a zero.");
				Err(Errno::Inval)
			}
		}
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

#[derive(Debug)]
pub(crate) struct UhyveDirectory {
	prefix: Option<String>,
}

impl UhyveDirectory {
	pub const fn new(prefix: Option<String>) -> Self {
		UhyveDirectory { prefix }
	}

	fn traversal_path(&self, components: &[&str]) -> CString {
		let prefix_deref = self.prefix.as_deref();
		let components_with_prefix = prefix_deref.iter().chain(components.iter().rev());
		// Unlike src/fs/fuse.rs, we skip the first element here so as to not prepend / before /root
		let path: String = components_with_prefix
			.flat_map(|component| ["/", component])
			.skip(1)
			.collect();
		if path.is_empty() {
			CString::new("/").unwrap()
		} else {
			CString::new(path).unwrap()
		}
	}
}

impl VfsNode for UhyveDirectory {
	/// Returns the node type
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn traverse_stat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(Errno::Nosys)
	}

	fn traverse_lstat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(Errno::Nosys)
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
		let path = self.traversal_path(components);

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
		let ret = open_params.ret; // circumvent packed field access
		match ret {
			// Assumption: Uhyve will never return a standard stream.
			ret if ret >= 0 => Ok(Arc::new(async_lock::RwLock::new(UhyveFileHandle::new(ret)))),
			_ => Err(ret.abs().try_into().unwrap()),
		}
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> io::Result<()> {
		let path = self.traversal_path(components);

		let mut unlink_params = UnlinkParams {
			name: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(path.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			ret: -1,
		};
		uhyve_hypercall(Hypercall::FileUnlink(&mut unlink_params));
		let ret = unlink_params.ret; // circumvent packed field access
		match ret {
			0 => Ok(()),
			_ => Err(unlink_params.ret.abs().try_into().unwrap()),
		}
	}

	fn traverse_rmdir(&self, _components: &mut Vec<&str>) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	fn traverse_mkdir(
		&self,
		_components: &mut Vec<&str>,
		_mode: AccessPermission,
	) -> io::Result<()> {
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
