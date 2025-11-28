// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Implements basic functions to realize a simple in-memory file system

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
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
	pub attr: FileAttr,
}

impl RomFileInner {
	pub fn new(data: &'static [u8], attr: FileAttr) -> Self {
		Self { data, attr }
	}
}

#[derive(Debug)]
struct RomFileInterface {
	/// Position within the file
	pos: Mutex<usize>,
	/// File content
	inner: Arc<RwLock<RomFileInner>>,
}

#[async_trait]
impl ObjectInterface for RomFileInterface {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let len = self.inner.read().await.data.len();
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
			let mut guard = self.inner.write().await;
			guard.attr.st_atim = t;
		}

		let vec = self.inner.read().await.data;
		let mut pos_guard = self.pos.lock().await;
		let pos = *pos_guard;

		if pos >= vec.len() {
			return Ok(0);
		}

		let len = if vec.len() - pos < buf.len() {
			vec.len() - pos
		} else {
			buf.len()
		};

		buf[..len].copy_from_slice(&vec[pos..pos + len]);
		*pos_guard = pos + len;

		Ok(len)
	}

	async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize> {
		let guard = self.inner.read().await;
		let mut pos_guard = self.pos.lock().await;

		let new_pos: isize = if whence == SeekWhence::Set {
			if offset < 0 {
				return Err(Errno::Inval);
			}

			offset
		} else if whence == SeekWhence::End {
			guard.data.len() as isize + offset
		} else if whence == SeekWhence::Cur {
			(*pos_guard as isize) + offset
		} else {
			return Err(Errno::Inval);
		};

		if new_pos <= isize::try_from(guard.data.len()).unwrap() {
			*pos_guard = new_pos.try_into().unwrap();
			Ok(new_pos)
		} else {
			Err(Errno::Badf)
		}
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let guard = self.inner.read().await;
		Ok(guard.attr)
	}
}

impl RomFileInterface {
	pub fn new(inner: Arc<RwLock<RomFileInner>>) -> Self {
		Self {
			pos: Mutex::new(0),
			inner,
		}
	}

	pub fn len(&self) -> usize {
		block_on(async { Ok(self.inner.read().await.data.len()) }, None).unwrap()
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

#[derive(Debug)]
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

		let new_pos: isize = if whence == SeekWhence::Set {
			if offset < 0 {
				return Err(Errno::Inval);
			}

			offset
		} else if whence == SeekWhence::End {
			guard.data.len() as isize + offset
		} else if whence == SeekWhence::Cur {
			(*pos_guard as isize) + offset
		} else {
			return Err(Errno::Inval);
		};

		if new_pos > isize::try_from(guard.data.len()).unwrap() {
			guard.data.resize(new_pos.try_into().unwrap(), 0);
			guard.attr.st_size = guard.data.len().try_into().unwrap();
		}
		*pos_guard = new_pos.try_into().unwrap();

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

	pub fn len(&self) -> usize {
		block_on(async { Ok(self.inner.read().await.data.len()) }, None).unwrap()
	}
}

#[derive(Debug)]
pub(crate) struct RomFile {
	data: Arc<RwLock<RomFileInner>>,
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
		block_on(async { Ok(self.data.read().await.attr) }, None)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(Errno::Badf)
		}
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(Errno::Badf)
		}
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
			data: Arc::new(RwLock::new(RomFileInner::new(data, attr))),
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

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(Errno::Badf)
		}
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(Errno::Badf)
		}
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

#[derive(Debug)]
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

	async fn async_traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				let mut guard = self.inner.write().await;
				if let Some(file) = guard.get(&node_name) {
					if opt.contains(OpenOption::O_DIRECTORY)
						&& file.get_kind() != NodeKind::Directory
					{
						return Err(Errno::Notdir);
					}

					if file.get_kind() == NodeKind::File || file.get_kind() == NodeKind::Directory {
						return file.get_object();
					} else {
						return Err(Errno::Noent);
					}
				} else if opt.contains(OpenOption::O_CREAT) {
					let file = Box::new(RamFile::new(mode));
					guard.insert(node_name, file.clone());
					return Ok(Arc::new(RwLock::new(RamFileInterface::new(
						file.data.clone(),
					))));
				} else {
					return Err(Errno::Noent);
				}
			}

			if let Some(directory) = self.inner.read().await.get(&node_name) {
				return directory.traverse_open(components, opt, mode);
			}
		}

		Err(Errno::Noent)
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

	fn traverse_mkdir(&self, components: &mut Vec<&str>, mode: AccessPermission) -> io::Result<()> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						return directory.traverse_mkdir(components, mode);
					}

					if components.is_empty() {
						self.inner
							.write()
							.await
							.insert(node_name, Box::new(MemDirectory::new(mode)));
						return Ok(());
					}
				}

				Err(Errno::Badf)
			},
			None,
		)
	}

	fn traverse_rmdir(&self, components: &mut Vec<&str>) -> io::Result<()> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty() {
						let mut guard = self.inner.write().await;

						let obj = guard.remove(&node_name).ok_or(Errno::Noent)?;
						if obj.get_kind() == NodeKind::Directory {
							return Ok(());
						} else {
							guard.insert(node_name, obj);
							return Err(Errno::Notdir);
						}
					} else if let Some(directory) = self.inner.read().await.get(&node_name) {
						return directory.traverse_rmdir(components);
					}
				}

				Err(Errno::Badf)
			},
			None,
		)
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> io::Result<()> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty() {
						let mut guard = self.inner.write().await;

						let obj = guard.remove(&node_name).ok_or(Errno::Noent)?;
						if obj.get_kind() == NodeKind::File {
							return Ok(());
						} else {
							guard.insert(node_name, obj);
							return Err(Errno::Isdir);
						}
					} else if let Some(directory) = self.inner.read().await.get(&node_name) {
						return directory.traverse_unlink(components);
					}
				}

				Err(Errno::Badf)
			},
			None,
		)
	}

	fn traverse_readdir(&self, components: &mut Vec<&str>) -> io::Result<Vec<DirectoryEntry>> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						directory.traverse_readdir(components)
					} else {
						Err(Errno::Badf)
					}
				} else {
					let mut entries: Vec<DirectoryEntry> = Vec::new();
					for name in self.inner.read().await.keys() {
						entries.push(DirectoryEntry::new(name.clone()));
					}

					Ok(entries)
				}
			},
			None,
		)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty()
						&& let Some(node) = self.inner.read().await.get(&node_name)
					{
						return node.get_file_attributes();
					}

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						directory.traverse_lstat(components)
					} else {
						Err(Errno::Badf)
					}
				} else {
					Err(Errno::Nosys)
				}
			},
			None,
		)
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> io::Result<FileAttr> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty()
						&& let Some(node) = self.inner.read().await.get(&node_name)
					{
						return node.get_file_attributes();
					}

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						directory.traverse_stat(components)
					} else {
						Err(Errno::Badf)
					}
				} else {
					Err(Errno::Nosys)
				}
			},
			None,
		)
	}

	fn traverse_mount(
		&self,
		components: &mut Vec<&str>,
		obj: Box<dyn VfsNode + core::marker::Send + core::marker::Sync>,
	) -> io::Result<()> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						return directory.traverse_mount(components, obj);
					}

					if components.is_empty() {
						self.inner.write().await.insert(node_name, obj);
						return Ok(());
					}
				}

				Err(Errno::Badf)
			},
			None,
		)
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> io::Result<Arc<RwLock<dyn ObjectInterface>>> {
		block_on(self.async_traverse_open(components, opt, mode), None)
	}

	fn traverse_create_file(
		&self,
		components: &mut Vec<&str>,
		data: &'static [u8],
		mode: AccessPermission,
	) -> io::Result<()> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let name = String::from(component);

					if components.is_empty() {
						let file = RomFile::new(data, mode);
						self.inner.write().await.insert(name, Box::new(file));
						return Ok(());
					}

					if let Some(directory) = self.inner.read().await.get(&name) {
						return directory.traverse_create_file(components, data, mode);
					}
				}

				Err(Errno::Noent)
			},
			None,
		)
	}
}
