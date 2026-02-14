//! Implements basic functions to realize a simple in-memory file system

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::{mem, ptr};

use align_address::Align;
use async_lock::{Mutex, RwLock};
use async_trait::async_trait;

use crate::errno::Errno;
use crate::executor::block_on;
use crate::fd::{AccessPermission, ObjectInterface, OpenOption, PollEvent};
use crate::fs::{DirectoryEntry, FileAttr, FileType, NodeKind, SeekWhence, VfsNode};
use crate::syscalls::Dirent64;
use crate::time::timespec;
use crate::{arch, io};

#[derive(Debug)]
pub(crate) struct RomFileInner {
	pub data: &'static [u8],
	pub attr: RwLock<FileAttr>,
}

impl RomFileInner {
	pub fn new(data: &'static [u8], attr: FileAttr) -> Self {
		Self {
			data,
			attr: RwLock::new(attr),
		}
	}
}

struct RomFileInterface {
	/// Position within the file
	pos: Mutex<usize>,
	/// File content
	inner: Arc<RomFileInner>,
}

#[async_trait]
impl ObjectInterface for RomFileInterface {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let len = self.inner.data.len();
		let pos = *self.pos.lock().await;

		let ret = if pos < len {
			event.intersection(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND)
		} else {
			PollEvent::empty()
		};

		Ok(ret)
	}

	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		{
			let microseconds = arch::kernel::systemtime::now_micros();
			let t = timespec::from_usec(microseconds as i64);
			self.inner.attr.write().await.st_atim = t;
		}

		let vec = self.inner.data;
		let mut pos_guard = self.pos.lock().await;
		let pos = *pos_guard;

		if pos >= vec.len() {
			return Ok(0);
		}

		let len = (vec.len() - pos).min(buf.len());
		buf[..len].copy_from_slice(&vec[pos..pos + len]);
		*pos_guard = pos + len;

		Ok(len)
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		// NOTE: Allocations can never be larger than `isize::MAX` bytes.
		let data_len = self.inner.data.len() as isize;

		let mut pos_guard = self.pos.lock().await;

		let new_pos = match whence {
			SeekWhence::Set => offset,
			SeekWhence::Cur => (*pos_guard as isize)
				.checked_add(offset)
				.ok_or(Errno::Overflow)?,
			SeekWhence::End => data_len.checked_add(offset).ok_or(Errno::Overflow)?,
			_ => return Err(Errno::Inval),
		};

		if !(0..=data_len).contains(&new_pos) {
			return Err(Errno::Inval);
		}

		*pos_guard = new_pos as usize;
		Ok(new_pos)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		Ok(*self.inner.attr.read().await)
	}
}

impl RomFileInterface {
	pub fn new(inner: Arc<RomFileInner>) -> Self {
		Self {
			pos: Mutex::new(0),
			inner,
		}
	}
}

#[derive(Debug)]
pub(crate) struct RamFileInner {
	pub data: Vec<u8>,
	pub attr: FileAttr,
}

impl RamFileInner {
	pub fn new(attr: FileAttr) -> Self {
		Self {
			data: Vec::new(),
			attr,
		}
	}
}

pub struct RamFileInterface {
	/// Position within the file
	pos: Mutex<usize>,
	/// File content
	inner: Arc<RwLock<RamFileInner>>,
}

#[async_trait]
impl ObjectInterface for RamFileInterface {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let len = self.inner.read().await.data.len();
		let pos = *self.pos.lock().await;

		let mut available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;

		if pos < len {
			available.insert(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND);
		}

		Ok(event & available)
	}

	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		{
			let microseconds = arch::kernel::systemtime::now_micros();
			let t = timespec::from_usec(microseconds as i64);
			let mut guard = self.inner.write().await;
			guard.attr.st_atim = t;
		}

		let guard = self.inner.read().await;
		let mut pos_guard = self.pos.lock().await;
		let pos = *pos_guard;

		if pos >= guard.data.len() {
			return Ok(0);
		}

		let len = if guard.data.len() - pos < buf.len() {
			guard.data.len() - pos
		} else {
			buf.len()
		};

		buf[..len].copy_from_slice(&guard.data[pos..pos + len]);
		*pos_guard = pos + len;

		Ok(len)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let microseconds = arch::kernel::systemtime::now_micros();
		let t = timespec::from_usec(microseconds as i64);
		let mut guard = self.inner.write().await;
		let mut pos_guard = self.pos.lock().await;
		let pos = *pos_guard;

		if pos + buf.len() > guard.data.len() {
			guard.data.resize(pos + buf.len(), 0);
			guard.attr.st_size = guard.data.len().try_into().unwrap();
		}

		guard.attr.st_atim = t;
		guard.attr.st_mtim = t;
		guard.attr.st_ctim = t;

		guard.data[pos..pos + buf.len()].copy_from_slice(buf);
		*pos_guard = pos + buf.len();

		Ok(buf.len())
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		let mut guard = self.inner.write().await;
		let mut pos_guard = self.pos.lock().await;

		// NOTE: Allocations can never be larger than `isize::MAX` bytes.
		let data_len = guard.data.len() as isize;

		let new_pos = match whence {
			SeekWhence::Set => offset,
			SeekWhence::Cur => (*pos_guard as isize)
				.checked_add(offset)
				.ok_or(Errno::Overflow)?,
			SeekWhence::End => data_len.checked_add(offset).ok_or(Errno::Overflow)?,
			_ => return Err(Errno::Inval),
		};

		if new_pos < 0 {
			return Err(Errno::Inval);
		}

		if new_pos > data_len {
			guard.data.resize(new_pos.try_into().unwrap(), 0);
			guard.attr.st_size = guard.data.len().try_into().unwrap();
		}
		*pos_guard = new_pos as usize;

		Ok(new_pos)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let guard = self.inner.read().await;
		Ok(guard.attr)
	}

	async fn truncate(&self, size: usize) -> io::Result<()> {
		let mut guard = self.inner.write().await;
		guard.data.resize(size, 0);
		guard.attr.st_size = size as i64;
		Ok(())
	}

	async fn chmod(&self, access_permission: AccessPermission) -> io::Result<()> {
		let mut guard = self.inner.write().await;
		guard.attr.st_mode = access_permission;
		Ok(())
	}
}

impl RamFileInterface {
	pub fn new(inner: Arc<RwLock<RamFileInner>>) -> Self {
		Self {
			pos: Mutex::new(0),
			inner,
		}
	}
}

#[derive(Debug)]
pub(crate) struct RomFile {
	data: Arc<RomFileInner>,
}

impl VfsNode for RomFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		Ok(Arc::new(RwLock::new(RomFileInterface::new(
			self.data.clone(),
		))))
	}

	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		block_on(async { Ok(*self.data.attr.read().await) }, None)
	}

	fn traverse_lstat(&self, path: &str) -> io::Result<FileAttr> {
		if !path.is_empty() {
			return Err(Errno::Badf);
		}

		self.get_file_attributes()
	}

	fn traverse_stat(&self, path: &str) -> io::Result<FileAttr> {
		if !path.is_empty() {
			return Err(Errno::Badf);
		}

		self.get_file_attributes()
	}
}

impl RomFile {
	pub fn new(data: &'static [u8], mode: AccessPermission) -> Self {
		let microseconds = arch::kernel::systemtime::now_micros();
		let t = timespec::from_usec(microseconds as i64);
		let attr = FileAttr {
			st_size: data.len().try_into().unwrap(),
			st_mode: mode | AccessPermission::S_IFREG,
			st_atim: t,
			st_mtim: t,
			st_ctim: t,
			..Default::default()
		};

		Self {
			data: Arc::new(RomFileInner::new(data, attr)),
		}
	}
}

#[derive(Debug, Clone)]
pub(crate) struct RamFile {
	data: Arc<RwLock<RamFileInner>>,
}

impl VfsNode for RamFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		Ok(Arc::new(RwLock::new(RamFileInterface::new(
			self.data.clone(),
		))))
	}

	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		block_on(async { Ok(self.data.read().await.attr) }, None)
	}

	fn traverse_lstat(&self, path: &str) -> io::Result<FileAttr> {
		if !path.is_empty() {
			return Err(Errno::Badf);
		}

		self.get_file_attributes()
	}

	fn traverse_stat(&self, path: &str) -> io::Result<FileAttr> {
		if !path.is_empty() {
			return Err(Errno::Badf);
		}

		self.get_file_attributes()
	}
}

impl RamFile {
	pub fn new(mode: AccessPermission) -> Self {
		let microseconds = arch::kernel::systemtime::now_micros();
		let t = timespec::from_usec(microseconds as i64);
		let attr = FileAttr {
			st_mode: mode | AccessPermission::S_IFREG,
			st_atim: t,
			st_mtim: t,
			st_ctim: t,
			..Default::default()
		};

		Self {
			data: Arc::new(RwLock::new(RamFileInner::new(attr))),
		}
	}
}

pub struct MemDirectoryInterface {
	/// Directory entries
	inner: Arc<RwLock<BTreeMap<String, Box<dyn VfsNode>>>>,
	read_idx: Mutex<usize>,
}

impl MemDirectoryInterface {
	pub fn new(inner: Arc<RwLock<BTreeMap<String, Box<dyn VfsNode>>>>) -> Self {
		Self {
			inner,
			read_idx: Mutex::new(0),
		}
	}
}

#[async_trait]
impl ObjectInterface for MemDirectoryInterface {
	async fn getdents(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		let mut buf_offset: usize = 0;
		let mut ret = 0;
		let mut read_idx = self.read_idx.lock().await;
		for name in self.inner.read().await.keys().skip(*read_idx) {
			let namelen = name.len();

			let dirent_len = mem::offset_of!(Dirent64, d_name) + namelen + 1;
			let next_dirent = (buf_offset + dirent_len).align_up(align_of::<Dirent64>());

			if next_dirent > buf.len() {
				// target buffer full -> we return the nr. of bytes written (like linux does)
				break;
			}

			*read_idx += 1;

			// could be replaced with slice_as_ptr once maybe_uninit_slice is stabilized.
			let target_dirent = buf[buf_offset].as_mut_ptr().cast::<Dirent64>();

			unsafe {
				target_dirent.write(Dirent64 {
					d_ino: 1, // TODO: we don't have inodes in the mem filesystem. Maybe this could lead to problems
					d_off: 0,
					d_reclen: (dirent_len.align_up(align_of::<Dirent64>()))
						.try_into()
						.unwrap(),
					d_type: FileType::Unknown, // TODO: Proper filetype
					d_name: PhantomData {},
				});
				let nameptr = ptr::from_mut(&mut (*(target_dirent)).d_name).cast::<u8>();
				nameptr.copy_from_nonoverlapping(name.as_bytes().as_ptr().cast::<u8>(), namelen);
				nameptr.add(namelen).write(0); // zero termination
			}

			buf_offset = next_dirent;
			ret = buf_offset;
		}
		Ok(ret)
	}

	/// lseek for a directory entry is the equivalent for seekdir on linux. But on Hermit this is
	/// logically the same operation, so we can just use the same fn in the backend.
	/// Any other offset than 0 is not supported. (Mostly because it doesn't make any sense, as
	/// userspace applications have no way of knowing valid offsets)
	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		if whence != SeekWhence::Set && offset != 0 {
			error!("Invalid offset for directory lseek ({offset})");
			return Err(Errno::Inval);
		}
		*self.read_idx.lock().await = offset as usize;
		Ok(offset)
	}
}

#[derive(Debug)]
pub(crate) struct MemDirectory {
	inner: Arc<RwLock<BTreeMap<String, Box<dyn VfsNode>>>>,
	attr: FileAttr,
}

impl MemDirectory {
	pub fn new(mode: AccessPermission) -> Self {
		let microseconds = arch::kernel::systemtime::now_micros();
		let t = timespec::from_usec(microseconds as i64);

		Self {
			inner: Arc::new(RwLock::new(BTreeMap::new())),
			attr: FileAttr {
				st_mode: mode | AccessPermission::S_IFDIR,
				st_atim: t,
				st_mtim: t,
				st_ctim: t,
				..Default::default()
			},
		}
	}

	async fn async_traverse_open(
		&self,
		path: &str,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		let (component, rest) = path.split_once("/").unwrap_or((path, ""));

		if !rest.is_empty() {
			let inner = self.inner.read().await;
			let directory = inner.get(component).ok_or(Errno::Noent)?;
			return directory.traverse_open(rest, opt, mode);
		}

		let mut inner = self.inner.write().await;
		let Some(file) = inner.get(component) else {
			if opt.contains(OpenOption::O_CREAT) {
				let file = Box::new(RamFile::new(mode));
				inner.insert(component.to_owned(), file.clone());
				let file = Arc::new(RwLock::new(RamFileInterface::new(file.data.clone())));
				return Ok(file);
			}

			return Err(Errno::Noent);
		};

		if opt.contains(OpenOption::O_DIRECTORY) && file.get_kind() != NodeKind::Directory {
			return Err(Errno::Notdir);
		}

		if file.get_kind() != NodeKind::File && file.get_kind() != NodeKind::Directory {
			return Err(Errno::Noent);
		}

		file.get_object()
	}
}

impl VfsNode for MemDirectory {
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn get_object(&self) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		Ok(Arc::new(RwLock::new(MemDirectoryInterface::new(
			self.inner.clone(),
		))))
	}

	fn get_file_attributes(&self) -> io::Result<FileAttr> {
		Ok(self.attr)
	}

	fn traverse_mkdir(&self, path: &str, mode: AccessPermission) -> io::Result<()> {
		block_on(
			async {
				let (component, rest) = path.split_once("/").unwrap_or((path, ""));

				if let Some(directory) = self.inner.read().await.get(component) {
					return directory.traverse_mkdir(rest, mode);
				}

				if !rest.is_empty() {
					return Err(Errno::Badf);
				}

				self.inner
					.write()
					.await
					.insert(component.to_owned(), Box::new(MemDirectory::new(mode)));
				Ok(())
			},
			None,
		)
	}

	fn traverse_rmdir(&self, path: &str) -> io::Result<()> {
		block_on(
			async {
				let (component, rest) = path.split_once("/").unwrap_or((path, ""));

				if !rest.is_empty() {
					let inner = &*self.inner.read().await;
					let directory = inner.get(component).ok_or(Errno::Badf)?;
					return directory.traverse_rmdir(rest);
				}

				let mut guard = self.inner.write().await;

				let obj = guard.remove(component).ok_or(Errno::Noent)?;
				if obj.get_kind() != NodeKind::Directory {
					guard.insert(component.to_owned(), obj);
					return Err(Errno::Notdir);
				}

				Ok(())
			},
			None,
		)
	}

	fn traverse_unlink(&self, path: &str) -> io::Result<()> {
		block_on(
			async {
				let (component, rest) = path.split_once("/").unwrap_or((path, ""));

				if !rest.is_empty() {
					let inner = self.inner.read().await;
					let directory = inner.get(component).ok_or(Errno::Badf)?;
					return directory.traverse_unlink(rest);
				}

				let mut guard = self.inner.write().await;

				let obj = guard.remove(component).ok_or(Errno::Noent)?;
				if obj.get_kind() != NodeKind::File {
					guard.insert(component.to_owned(), obj);
					return Err(Errno::Isdir);
				}

				Ok(())
			},
			None,
		)
	}

	fn traverse_readdir(&self, path: &str) -> io::Result<Vec<DirectoryEntry>> {
		block_on(
			async {
				if let Some((component, rest)) = path.split_once("/") {
					let inner = self.inner.read().await;
					let directory = inner.get(component).ok_or(Errno::Badf)?;
					return directory.traverse_readdir(rest);
				};

				let mut entries = Vec::new();
				for name in self.inner.read().await.keys() {
					entries.push(DirectoryEntry::new(name.clone()));
				}

				Ok(entries)
			},
			None,
		)
	}

	fn traverse_lstat(&self, path: &str) -> io::Result<FileAttr> {
		block_on(
			async {
				let (component, rest) = path.split_once("/").unwrap_or((path, ""));

				if !rest.is_empty() {
					let inner = self.inner.read().await;
					let directory = inner.get(component).ok_or(Errno::Badf)?;
					return directory.traverse_lstat(rest);
				}

				let inner = self.inner.read().await;
				let node = inner.get(component).ok_or(Errno::Badf)?;
				node.get_file_attributes()
			},
			None,
		)
	}

	fn traverse_stat(&self, path: &str) -> io::Result<FileAttr> {
		block_on(
			async {
				let (component, rest) = path.split_once("/").unwrap_or((path, ""));

				if !rest.is_empty() {
					let inner = self.inner.read().await;
					let directory = inner.get(component).ok_or(Errno::Badf)?;
					return directory.traverse_stat(rest);
				}

				let inner = self.inner.read().await;
				let node = inner.get(component).ok_or(Errno::Badf)?;
				node.get_file_attributes()
			},
			None,
		)
	}

	fn traverse_mount(&self, path: &str, obj: Box<dyn VfsNode>) -> io::Result<()> {
		block_on(
			async {
				let (component, rest) = path.split_once("/").unwrap_or((path, ""));

				if let Some(directory) = self.inner.read().await.get(component) {
					return directory.traverse_mount(rest, obj);
				}

				if !rest.is_empty() {
					return Err(Errno::Badf);
				}

				self.inner.write().await.insert(component.to_owned(), obj);
				Ok(())
			},
			None,
		)
	}

	fn traverse_open(
		&self,
		path: &str,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		block_on(self.async_traverse_open(path, opt, mode), None)
	}

	fn traverse_create_file(
		&self,
		path: &str,
		data: &'static [u8],
		mode: AccessPermission,
	) -> io::Result<()> {
		block_on(
			async {
				let (component, rest) = path.split_once("/").unwrap_or((path, ""));

				if component.is_empty() {
					return Err(Errno::Noent);
				}

				if !rest.is_empty() {
					let inner = self.inner.read().await;
					let directory = inner.get(component).ok_or(Errno::Noent)?;
					return directory.traverse_create_file(rest, data, mode);
				}

				let file = RomFile::new(data, mode);
				self.inner
					.write()
					.await
					.insert(component.to_owned(), Box::new(file));
				Ok(())
			},
			None,
		)
	}
}
