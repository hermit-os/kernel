//! Implements basic functions to realize a simple in-memory file system

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::collections::btree_map::Entry;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::mem::{MaybeUninit, offset_of};

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

		if (0..=data_len).contains(&new_pos) {
			*pos_guard = new_pos as usize;
			Ok(new_pos)
		} else {
			Err(Errno::Inval)
		}
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

#[derive(Clone, Debug)]
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

	fn dup(&self) -> Box<dyn VfsNode> {
		Box::new(self.clone())
	}

	fn traverse_once(&self, _component: &str) -> io::Result<Box<dyn VfsNode>> {
		Err(Errno::Badf)
	}

	fn lstat(&self) -> io::Result<FileAttr> {
		self.get_file_attributes()
	}

	fn stat(&self) -> io::Result<FileAttr> {
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

	fn dup(&self) -> Box<dyn VfsNode> {
		Box::new(self.clone())
	}

	fn traverse_once(&self, _component: &str) -> io::Result<Box<dyn VfsNode>> {
		Err(Errno::Badf)
	}

	fn lstat(&self) -> io::Result<FileAttr> {
		self.get_file_attributes()
	}

	fn stat(&self) -> io::Result<FileAttr> {
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
	inner:
		Arc<RwLock<BTreeMap<String, Box<dyn VfsNode + core::marker::Send + core::marker::Sync>>>>,
	read_idx: Mutex<usize>,
}

impl MemDirectoryInterface {
	pub fn new(
		inner: Arc<
			RwLock<BTreeMap<String, Box<dyn VfsNode + core::marker::Send + core::marker::Sync>>>,
		>,
	) -> Self {
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

			let dirent_len = offset_of!(Dirent64, d_name) + namelen + 1;
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
				let nameptr = core::ptr::from_mut(&mut (*(target_dirent)).d_name).cast::<u8>();
				core::ptr::copy_nonoverlapping(
					name.as_bytes().as_ptr().cast::<u8>(),
					nameptr,
					namelen,
				);
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
	inner:
		Arc<RwLock<BTreeMap<String, Box<dyn VfsNode + core::marker::Send + core::marker::Sync>>>>,
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

	fn dup(&self) -> Box<dyn VfsNode> {
		Box::new(MemDirectory {
			inner: Arc::clone(&self.inner),
			attr: self.attr,
		})
	}

	fn traverse_once(&self, component: &str) -> io::Result<Box<dyn VfsNode>> {
		block_on(
			async {
				if let Some(directory) = self.inner.read().await.get(component) {
					Ok(directory.dup())
				} else {
					Err(Errno::Badf)
				}
			},
			None,
		)
	}

	fn mkdir(&self, component: &str, mode: AccessPermission) -> io::Result<()> {
		block_on(
			async {
				self.inner
					.write()
					.await
					.insert(component.to_owned(), Box::new(MemDirectory::new(mode)));
				Ok(())
			},
			None,
		)
	}

	fn rmdir(&self, component: &str) -> io::Result<()> {
		block_on(
			async {
				let mut guard = self.inner.write().await;
				let obj = guard.remove(component).ok_or(Errno::Noent)?;
				if obj.get_kind() == NodeKind::Directory {
					Ok(())
				} else {
					guard.insert(component.to_owned(), obj);
					Err(Errno::Notdir)
				}
			},
			None,
		)
	}

	fn unlink(&self, component: &str) -> io::Result<()> {
		block_on(
			async {
				let mut guard = self.inner.write().await;
				let obj = guard.remove(component).ok_or(Errno::Noent)?;
				if obj.get_kind() == NodeKind::File {
					Ok(())
				} else {
					guard.insert(component.to_owned(), obj);
					Err(Errno::Isdir)
				}
			},
			None,
		)
	}

	fn readdir(&self) -> io::Result<Vec<DirectoryEntry>> {
		block_on(
			async {
				Ok(self
					.inner
					.read()
					.await
					.keys()
					.map(|name| DirectoryEntry::new(name.clone()))
					.collect())
			},
			None,
		)
	}

	fn lstat(&self) -> io::Result<FileAttr> {
		self.get_file_attributes()
	}

	fn stat(&self) -> io::Result<FileAttr> {
		self.get_file_attributes()
	}

	fn mount(&self, component: &str, obj: Box<dyn VfsNode + Send + Sync>) -> io::Result<()> {
		block_on(
			async {
				let mut guard = self.inner.write().await;
				match guard.entry(component.to_owned()) {
					Entry::Vacant(vac) => {
						vac.insert(obj);
						Ok(())
					}
					Entry::Occupied(_) => Err(Errno::Badf),
				}
			},
			None,
		)
	}

	fn open(
		&self,
		component: &str,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		block_on(
			async {
				let mut guard = self.inner.write().await;
				if let Some(file) = guard.get(component) {
					if opt.contains(OpenOption::O_DIRECTORY)
						&& file.get_kind() != NodeKind::Directory
					{
						Err(Errno::Notdir)
					} else if file.get_kind() == NodeKind::File
						|| file.get_kind() == NodeKind::Directory
					{
						file.get_object()
					} else {
						Err(Errno::Noent)
					}
				} else if opt.contains(OpenOption::O_CREAT) {
					let file = Box::new(RamFile::new(mode));
					guard.insert(component.to_owned(), file.clone());
					let fd: Arc<RwLock<dyn ObjectInterface>> =
						Arc::new(RwLock::new(RamFileInterface::new(file.data.clone())));
					Ok(fd)
				} else {
					Err(Errno::Noent)
				}
			},
			None,
		)
	}

	fn create_file(
		&self,
		component: &str,
		data: &'static [u8],
		mode: AccessPermission,
	) -> io::Result<()> {
		block_on(
			async {
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
