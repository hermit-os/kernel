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
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::slice;

use async_lock::{Mutex, RwLock};
use async_trait::async_trait;

use crate::arch;
use crate::executor::block_on;
use crate::fd::{AccessPermission, IoError, ObjectInterface, OpenOption, PollEvent};
use crate::fs::{DirectoryEntry, FileAttr, NodeKind, VfsNode};
use crate::time::timespec;

#[derive(Debug)]
pub(crate) struct RomFileInner {
	pub data: &'static [u8],
	pub attr: FileAttr,
}

impl RomFileInner {
	pub unsafe fn new(ptr: *const u8, length: usize, attr: FileAttr) -> Self {
		Self {
			data: unsafe { slice::from_raw_parts(ptr, length) },
			attr,
		}
	}
}

#[derive(Debug, Clone)]
struct RomFileInterface {
	/// Position within the file
	pos: Arc<Mutex<usize>>,
	/// File content
	inner: Arc<RwLock<RomFileInner>>,
}

#[async_trait]
impl ObjectInterface for RomFileInterface {
	async fn poll(&self, event: PollEvent) -> Result<PollEvent, IoError> {
		let len = self.inner.read().await.data.len();
		let pos = *self.pos.lock().await;

		let ret = if pos < len {
			event.intersection(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND)
		} else {
			PollEvent::empty()
		};

		Ok(ret)
	}

	async fn async_read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
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

		buf[0..len].clone_from_slice(&vec[pos..pos + len]);
		*pos_guard = pos + len;

		Ok(len)
	}
}

impl RomFileInterface {
	pub fn new(inner: Arc<RwLock<RomFileInner>>) -> Self {
		Self {
			pos: Arc::new(Mutex::new(0)),
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

#[derive(Debug, Clone)]
pub struct RamFileInterface {
	/// Position within the file
	pos: Arc<Mutex<usize>>,
	/// File content
	inner: Arc<RwLock<RamFileInner>>,
}

#[async_trait]
impl ObjectInterface for RamFileInterface {
	async fn poll(&self, event: PollEvent) -> Result<PollEvent, IoError> {
		let len = self.inner.read().await.data.len();
		let pos = *self.pos.lock().await;

		let mut available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;

		if pos < len {
			available.insert(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND);
		}

		Ok(event & available)
	}

	async fn async_read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
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

		buf[0..len].clone_from_slice(&guard.data[pos..pos + len]);
		*pos_guard = pos + len;

		Ok(len)
	}

	async fn async_write(&self, buf: &[u8]) -> Result<usize, IoError> {
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

		guard.data[pos..pos + buf.len()].clone_from_slice(buf);
		*pos_guard = pos + buf.len();

		Ok(buf.len())
	}
}

impl RamFileInterface {
	pub fn new(inner: Arc<RwLock<RamFileInner>>) -> Self {
		Self {
			pos: Arc::new(Mutex::new(0)),
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

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(RomFileInterface::new(self.data.clone())))
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		block_on(async { Ok(self.data.read().await.attr) }, None)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(IoError::EBADF)
		}
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(IoError::EBADF)
		}
	}
}

impl RomFile {
	pub unsafe fn new(ptr: *const u8, length: usize, mode: AccessPermission) -> Self {
		let microseconds = arch::kernel::systemtime::now_micros();
		let t = timespec::from_usec(microseconds as i64);
		let attr = FileAttr {
			st_size: length.try_into().unwrap(),
			st_mode: mode | AccessPermission::S_IFREG,
			st_atim: t,
			st_mtim: t,
			st_ctim: t,
			..Default::default()
		};

		Self {
			data: unsafe { Arc::new(RwLock::new(RomFileInner::new(ptr, length, attr))) },
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

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(RamFileInterface::new(self.data.clone())))
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		block_on(async { Ok(self.data.read().await.attr) }, None)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(IoError::EBADF)
		}
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			self.get_file_attributes()
		} else {
			Err(IoError::EBADF)
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

#[derive(Debug, Clone)]
pub struct MemDirectoryInterface {
	/// Directory entries
	inner:
		Arc<RwLock<BTreeMap<String, Box<dyn VfsNode + core::marker::Send + core::marker::Sync>>>>,
}

impl MemDirectoryInterface {
	pub fn new(
		inner: Arc<
			RwLock<BTreeMap<String, Box<dyn VfsNode + core::marker::Send + core::marker::Sync>>>,
		>,
	) -> Self {
		Self { inner }
	}
}

#[async_trait]
impl ObjectInterface for MemDirectoryInterface {
	fn readdir(&self) -> Result<Vec<DirectoryEntry>, IoError> {
		block_on(
			async {
				let mut entries: Vec<DirectoryEntry> = Vec::new();
				for name in self.inner.read().await.keys() {
					entries.push(DirectoryEntry::new(name.to_string()));
				}

				Ok(entries)
			},
			None,
		)
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
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				let mut guard = self.inner.write().await;
				if opt.contains(OpenOption::O_CREAT) || opt.contains(OpenOption::O_CREAT) {
					if guard.get(&node_name).is_some() {
						return Err(IoError::EEXIST);
					} else {
						let file = Box::new(RamFile::new(mode));
						guard.insert(node_name, file.clone());
						return Ok(Arc::new(RamFileInterface::new(file.data.clone())));
					}
				} else if let Some(file) = guard.get(&node_name) {
					if opt.contains(OpenOption::O_DIRECTORY)
						&& file.get_kind() != NodeKind::Directory
					{
						return Err(IoError::ENOTDIR);
					}

					if file.get_kind() == NodeKind::File || file.get_kind() == NodeKind::Directory {
						return file.get_object();
					} else {
						return Err(IoError::ENOENT);
					}
				} else {
					return Err(IoError::ENOENT);
				}
			}

			if let Some(directory) = self.inner.read().await.get(&node_name) {
				return directory.traverse_open(components, opt, mode);
			}
		}

		Err(IoError::ENOENT)
	}
}

impl VfsNode for MemDirectory {
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(MemDirectoryInterface::new(self.inner.clone())))
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		Ok(self.attr)
	}

	fn traverse_mkdir(
		&self,
		components: &mut Vec<&str>,
		mode: AccessPermission,
	) -> Result<(), IoError> {
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

				Err(IoError::EBADF)
			},
			None,
		)
	}

	fn traverse_rmdir(&self, components: &mut Vec<&str>) -> Result<(), IoError> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty() {
						let mut guard = self.inner.write().await;

						let obj = guard.remove(&node_name).ok_or(IoError::ENOENT)?;
						if obj.get_kind() == NodeKind::Directory {
							return Ok(());
						} else {
							guard.insert(node_name, obj);
							return Err(IoError::ENOTDIR);
						}
					} else if let Some(directory) = self.inner.read().await.get(&node_name) {
						return directory.traverse_rmdir(components);
					}
				}

				Err(IoError::EBADF)
			},
			None,
		)
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> Result<(), IoError> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty() {
						let mut guard = self.inner.write().await;

						let obj = guard.remove(&node_name).ok_or(IoError::ENOENT)?;
						if obj.get_kind() == NodeKind::File {
							return Ok(());
						} else {
							guard.insert(node_name, obj);
							return Err(IoError::EISDIR);
						}
					} else if let Some(directory) = self.inner.read().await.get(&node_name) {
						return directory.traverse_unlink(components);
					}
				}

				Err(IoError::EBADF)
			},
			None,
		)
	}

	fn traverse_readdir(&self, components: &mut Vec<&str>) -> Result<Vec<DirectoryEntry>, IoError> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						directory.traverse_readdir(components)
					} else {
						Err(IoError::EBADF)
					}
				} else {
					let mut entries: Vec<DirectoryEntry> = Vec::new();
					for name in self.inner.read().await.keys() {
						entries.push(DirectoryEntry::new(name.to_string()));
					}

					Ok(entries)
				}
			},
			None,
		)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty() {
						if let Some(node) = self.inner.read().await.get(&node_name) {
							return node.get_file_attributes();
						}
					}

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						directory.traverse_lstat(components)
					} else {
						Err(IoError::EBADF)
					}
				} else {
					Err(IoError::ENOSYS)
				}
			},
			None,
		)
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let node_name = String::from(component);

					if components.is_empty() {
						if let Some(node) = self.inner.read().await.get(&node_name) {
							return node.get_file_attributes();
						}
					}

					if let Some(directory) = self.inner.read().await.get(&node_name) {
						directory.traverse_stat(components)
					} else {
						Err(IoError::EBADF)
					}
				} else {
					Err(IoError::ENOSYS)
				}
			},
			None,
		)
	}

	fn traverse_mount(
		&self,
		components: &mut Vec<&str>,
		obj: Box<dyn VfsNode + core::marker::Send + core::marker::Sync>,
	) -> Result<(), IoError> {
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

				Err(IoError::EBADF)
			},
			None,
		)
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		block_on(self.async_traverse_open(components, opt, mode), None)
	}

	fn traverse_create_file(
		&self,
		components: &mut Vec<&str>,
		ptr: *const u8,
		length: usize,
		mode: AccessPermission,
	) -> Result<(), IoError> {
		block_on(
			async {
				if let Some(component) = components.pop() {
					let name = String::from(component);

					if components.is_empty() {
						let file = unsafe { RomFile::new(ptr, length, mode) };
						self.inner
							.write()
							.await
							.insert(name.to_string(), Box::new(file));
						return Ok(());
					}

					if let Some(directory) = self.inner.read().await.get(&name) {
						return directory.traverse_create_file(components, ptr, length, mode);
					}
				}

				Err(IoError::ENOENT)
			},
			None,
		)
	}
}
