use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use async_lock::Mutex;
use async_trait::async_trait;
use memory_addresses::VirtAddr;
use uhyve_interface::parameters::{
	CloseParams, LseekParams, OpenParams, ReadParams, UnlinkParams, WriteParams,
};
use uhyve_interface::{GuestPhysAddr, GuestVirtAddr, Hypercall};

use crate::arch::mm::paging;
use crate::env::is_uhyve;
use crate::fs::{
	self, AccessPermission, FileAttr, NodeKind, ObjectInterface, OpenOption, SeekWhence, VfsNode,
};
use crate::io;
use crate::syscalls::interfaces::uhyve::uhyve_hypercall;

#[derive(Debug)]
struct UhyveFileHandleInner(i32);

impl UhyveFileHandleInner {
	pub fn new(fd: i32) -> Self {
		Self(fd)
	}

	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		let mut read_params = ReadParams {
			fd: self.0,
			buf: GuestVirtAddr::new(buf.as_mut_ptr() as u64),
			len: buf.len(),
			ret: 0,
		};
		uhyve_hypercall(Hypercall::FileRead(&mut read_params));

		if read_params.ret >= 0 {
			Ok(read_params.ret.try_into().unwrap())
		} else {
			Err(io::Error::EIO)
		}
	}

	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		let write_params = WriteParams {
			fd: self.0,
			buf: GuestVirtAddr::new(buf.as_ptr() as u64),
			len: buf.len(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&write_params));

		Ok(write_params.len)
	}

	fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		let mut lseek_params = LseekParams {
			fd: self.0,
			offset,
			whence: num::ToPrimitive::to_i32(&whence).unwrap(),
		};
		uhyve_hypercall(Hypercall::FileLseek(&mut lseek_params));

		if lseek_params.offset >= 0 {
			Ok(lseek_params.offset)
		} else {
			Err(io::Error::EINVAL)
		}
	}
}

impl Drop for UhyveFileHandleInner {
	fn drop(&mut self) {
		let mut close_params = CloseParams { fd: self.0, ret: 0 };
		uhyve_hypercall(Hypercall::FileClose(&mut close_params));
		if close_params.ret != 0 {
			let ret = close_params.ret; // circumvent packed field access
			panic!("Can't close fd {} - return value {ret}", self.0,);
		}
	}
}

#[derive(Debug)]
struct UhyveFileHandle(pub Arc<Mutex<UhyveFileHandleInner>>);

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
pub(crate) struct UhyveDirectory;

impl UhyveDirectory {
	pub const fn new() -> Self {
		UhyveDirectory {}
	}
}

impl VfsNode for UhyveDirectory {
	/// Returns the node type
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn traverse_stat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(io::Error::ENOSYS)
	}

	fn traverse_lstat(&self, _components: &mut Vec<&str>) -> io::Result<FileAttr> {
		Err(io::Error::ENOSYS)
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<dyn ObjectInterface>> {
		let path: String = if components.is_empty() {
			"/\0".to_string()
		} else {
			let mut path: String = components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect();
			path.push('\0');
			path.remove(0);
			path
		};

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
			Ok(Arc::new(UhyveFileHandle::new(open_params.ret)))
		} else {
			Err(io::Error::EIO)
		}
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> io::Result<()> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

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
			Err(io::Error::EIO)
		}
	}

	fn traverse_rmdir(&self, _components: &mut Vec<&str>) -> io::Result<()> {
		Err(io::Error::ENOSYS)
	}

	fn traverse_mkdir(
		&self,
		_components: &mut Vec<&str>,
		_mode: AccessPermission,
	) -> io::Result<()> {
		Err(io::Error::ENOSYS)
	}
}

pub(crate) fn init() {
	info!("Try to initialize uhyve filesystem");
	if is_uhyve() {
		let mount_point = hermit_var_or!("UHYVE_MOUNT", "/root").to_string();
		info!("Mounting uhyve filesystem at {}", mount_point);
		fs::FILESYSTEM
			.get()
			.unwrap()
			.mount(&mount_point, Box::new(UhyveDirectory::new()))
			.expect("Mount failed. Duplicate mount_point?");
	}
}
