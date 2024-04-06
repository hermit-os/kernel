use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use core::ptr;

use async_lock::Mutex;
use async_trait::async_trait;
#[cfg(target_arch = "x86_64")]
use x86::io::outl;

use crate::arch::mm::{paging, PhysAddr, VirtAddr};
use crate::env::is_uhyve;
use crate::executor::block_on;
use crate::fd::IoError;
use crate::fs::{
	self, AccessPermission, FileAttr, NodeKind, ObjectInterface, OpenOption, SeekWhence, VfsNode,
};

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "x86_64")]
fn uhyve_send<T>(port: u16, data: &mut T) {
	let ptr = VirtAddr(ptr::from_mut(data).addr() as u64);
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	unsafe {
		outl(port, physical_address.as_u64() as u32);
	}
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "aarch64")]
fn uhyve_send<T>(port: u16, data: &mut T) {
	use core::arch::asm;

	let ptr = VirtAddr(ptr::from_mut(data).addr() as u64);
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	unsafe {
		asm!(
			"str x8, [{port}]",
			port = in(reg) u64::from(port),
			in("x8") physical_address.as_u64(),
			options(nostack),
		);
	}
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "riscv64")]
fn uhyve_send<T>(_port: u16, _data: &mut T) {
	todo!()
}

const UHYVE_PORT_WRITE: u16 = 0x400;
const UHYVE_PORT_OPEN: u16 = 0x440;
const UHYVE_PORT_CLOSE: u16 = 0x480;
const UHYVE_PORT_READ: u16 = 0x500;
const UHYVE_PORT_LSEEK: u16 = 0x580;
const UHYVE_PORT_UNLINK: u16 = 0x840;

#[repr(C, packed)]
struct SysOpen {
	name: PhysAddr,
	flags: i32,
	mode: u32,
	ret: i32,
}

impl SysOpen {
	fn new(name: VirtAddr, flags: i32, mode: u32) -> SysOpen {
		SysOpen {
			name: paging::virtual_to_physical(name).unwrap(),
			flags,
			mode,
			ret: -1,
		}
	}
}

#[repr(C, packed)]
struct SysClose {
	fd: i32,
	ret: i32,
}

impl SysClose {
	fn new(fd: i32) -> SysClose {
		SysClose { fd, ret: -1 }
	}
}

#[repr(C, packed)]
struct SysRead {
	fd: i32,
	buf: *const u8,
	len: usize,
	ret: isize,
}

impl SysRead {
	fn new(fd: i32, buf: *const u8, len: usize) -> SysRead {
		SysRead {
			fd,
			buf,
			len,
			ret: -1,
		}
	}
}

#[repr(C, packed)]
struct SysWrite {
	fd: i32,
	buf: *const u8,
	len: usize,
}

impl SysWrite {
	pub fn new(fd: i32, buf: *const u8, len: usize) -> SysWrite {
		SysWrite { fd, buf, len }
	}
}

#[repr(C, packed)]
struct SysLseek {
	pub fd: i32,
	pub offset: isize,
	pub whence: i32,
}

impl SysLseek {
	fn new(fd: i32, offset: isize, whence: SeekWhence) -> SysLseek {
		let whence: i32 = num::ToPrimitive::to_i32(&whence).unwrap();

		SysLseek { fd, offset, whence }
	}
}

#[repr(C, packed)]
struct SysUnlink {
	name: PhysAddr,
	ret: i32,
}

impl SysUnlink {
	fn new(name: VirtAddr) -> SysUnlink {
		SysUnlink {
			name: paging::virtual_to_physical(name).unwrap(),
			ret: -1,
		}
	}
}

#[derive(Debug)]
struct UhyveFileHandleInner(i32);

impl UhyveFileHandleInner {
	pub fn new(fd: i32) -> Self {
		Self(fd)
	}

	fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
		let mut sysread = SysRead::new(self.0, buf.as_mut_ptr(), buf.len());
		uhyve_send(UHYVE_PORT_READ, &mut sysread);

		if sysread.ret >= 0 {
			Ok(sysread.ret.try_into().unwrap())
		} else {
			Err(IoError::EIO)
		}
	}

	fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
		let mut syswrite = SysWrite::new(self.0, buf.as_ptr(), buf.len());
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		Ok(syswrite.len)
	}

	fn lseek(&self, offset: isize, whence: SeekWhence) -> Result<isize, IoError> {
		let mut syslseek = SysLseek::new(self.0, offset, whence);
		uhyve_send(UHYVE_PORT_LSEEK, &mut syslseek);

		if syslseek.offset >= 0 {
			Ok(syslseek.offset)
		} else {
			Err(IoError::EINVAL)
		}
	}
}

impl Drop for UhyveFileHandleInner {
	fn drop(&mut self) {
		let mut sysclose = SysClose::new(self.0);
		uhyve_send(UHYVE_PORT_CLOSE, &mut sysclose);
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
	async fn async_read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
		self.0.lock().await.read(buf)
	}

	async fn async_write(&self, buf: &[u8]) -> Result<usize, IoError> {
		self.0.lock().await.write(buf)
	}

	fn lseek(&self, offset: isize, whence: SeekWhence) -> Result<isize, IoError> {
		block_on(async { self.0.lock().await.lseek(offset, whence) }, None)
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

	fn traverse_stat(&self, _components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		Err(IoError::ENOSYS)
	}

	fn traverse_lstat(&self, _components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		Err(IoError::ENOSYS)
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
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

		let mut sysopen = SysOpen::new(VirtAddr(path.as_ptr() as u64), opt.bits(), mode.bits());
		uhyve_send(UHYVE_PORT_OPEN, &mut sysopen);

		if sysopen.ret > 0 {
			Ok(Arc::new(UhyveFileHandle::new(sysopen.ret)))
		} else {
			Err(IoError::EIO)
		}
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> core::result::Result<(), IoError> {
		let path: String = if components.is_empty() {
			"/".to_string()
		} else {
			components
				.iter()
				.rev()
				.map(|v| "/".to_owned() + v)
				.collect()
		};

		let mut sysunlink = SysUnlink::new(VirtAddr(path.as_ptr() as u64));
		uhyve_send(UHYVE_PORT_UNLINK, &mut sysunlink);

		if sysunlink.ret == 0 {
			Ok(())
		} else {
			Err(IoError::EIO)
		}
	}

	fn traverse_rmdir(&self, _components: &mut Vec<&str>) -> core::result::Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	fn traverse_mkdir(
		&self,
		_components: &mut Vec<&str>,
		_mode: AccessPermission,
	) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}
}

pub(crate) fn init() {
	info!("Try to initialize uhyve filesystem");
	if is_uhyve() {
		let mount_point = hermit_var_or!("UHYVE_MOUNT", "/root").to_string();
		info!("Mounting virtio-fs at {}", mount_point);
		fs::FILESYSTEM
			.get()
			.unwrap()
			.mount(&mount_point, Box::new(UhyveDirectory::new()))
			.expect("Mount failed. Duplicate mount_point?");
	}
}
