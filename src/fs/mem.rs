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

use async_trait::async_trait;
use hermit_sync::{RwSpinLock, SpinMutex};

use crate::arch;
use crate::fd::{AccessPermission, IoError, ObjectInterface, OpenOption, PollEvent};
use crate::fs::{DirectoryEntry, FileAttr, NodeKind, VfsNode};

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
	pos: Arc<SpinMutex<usize>>,
	/// File content
	inner: Arc<RwSpinLock<RomFileInner>>,
}

#[async_trait]
impl ObjectInterface for RomFileInterface {
	async fn poll(&self, event: PollEvent) -> Result<PollEvent, IoError> {
		let mut result: PollEvent = PollEvent::EMPTY;
		let len = self.inner.read().data.len();
		let pos_guard = self.pos.lock();
		let pos = *pos_guard;

		if event.contains(PollEvent::POLLIN) && pos < len {
			result.insert(PollEvent::POLLIN);
		} else if event.contains(PollEvent::POLLRDNORM) && pos < len {
			result.insert(PollEvent::POLLRDNORM);
		} else if event.contains(PollEvent::POLLRDBAND) && pos < len {
			result.insert(PollEvent::POLLRDBAND);
		}

		Ok(result)
	}

	fn read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
		{
			let microseconds = arch::kernel::systemtime::now_micros();
			let mut guard = self.inner.write();
			guard.attr.st_atime = microseconds / 1_000_000;
			guard.attr.st_atime_nsec = (microseconds % 1_000_000) * 1000;
		}

		let vec = self.inner.read().data;
		let mut pos_guard = self.pos.lock();
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
	pub fn new(inner: Arc<RwSpinLock<RomFileInner>>) -> Self {
		Self {
			pos: Arc::new(SpinMutex::new(0)),
			inner,
		}
	}

	pub fn len(&self) -> usize {
		self.inner.read().data.len()
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
	pos: Arc<SpinMutex<usize>>,
	/// File content
	inner: Arc<RwSpinLock<RamFileInner>>,
}

#[async_trait]
impl ObjectInterface for RamFileInterface {
	async fn poll(&self, event: PollEvent) -> Result<PollEvent, IoError> {
		let mut result: PollEvent = PollEvent::EMPTY;
		let len = self.inner.read().data.len();
		let pos_guard = self.pos.lock();
		let pos = *pos_guard;

		if event.contains(PollEvent::POLLIN) && pos < len {
			result.insert(PollEvent::POLLIN);
		} else if event.contains(PollEvent::POLLRDNORM) && pos < len {
			result.insert(PollEvent::POLLRDNORM);
		} else if event.contains(PollEvent::POLLRDBAND) && pos < len {
			result.insert(PollEvent::POLLRDBAND);
		} else if event.contains(PollEvent::POLLOUT) {
			result.insert(PollEvent::POLLOUT);
		} else if event.contains(PollEvent::POLLWRNORM) {
			result.insert(PollEvent::POLLWRNORM);
		} else if event.contains(PollEvent::POLLWRBAND) {
			result.insert(PollEvent::POLLWRBAND);
		}

		Ok(result)
	}

	fn read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
		{
			let microseconds = arch::kernel::systemtime::now_micros();
			let mut guard = self.inner.write();
			guard.attr.st_atime = microseconds / 1_000_000;
			guard.attr.st_atime_nsec = (microseconds % 1_000_000) * 1000;
		}

		let guard = self.inner.read();
		let mut pos_guard = self.pos.lock();
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

	fn write(&self, buf: &[u8]) -> Result<usize, IoError> {
		let microseconds = arch::kernel::systemtime::now_micros();
		let mut guard = self.inner.write();
		let mut pos_guard = self.pos.lock();
		let pos = *pos_guard;

		if pos + buf.len() > guard.data.len() {
			guard.data.resize(pos + buf.len(), 0);
			guard.attr.st_size = guard.data.len().try_into().unwrap();
		}
		guard.attr.st_atime = microseconds / 1_000_000;
		guard.attr.st_atime_nsec = (microseconds % 1_000_000) * 1000;
		guard.attr.st_mtime = guard.attr.st_atime;
		guard.attr.st_mtime_nsec = guard.attr.st_atime_nsec;
		guard.attr.st_ctime = guard.attr.st_atime;
		guard.attr.st_ctime_nsec = guard.attr.st_atime_nsec;

		guard.data[pos..pos + buf.len()].clone_from_slice(buf);
		*pos_guard = pos + buf.len();

		Ok(buf.len())
	}
}

impl RamFileInterface {
	pub fn new(inner: Arc<RwSpinLock<RamFileInner>>) -> Self {
		Self {
			pos: Arc::new(SpinMutex::new(0)),
			inner,
		}
	}

	pub fn len(&self) -> usize {
		self.inner.read().data.len()
	}
}

#[derive(Debug)]
pub(crate) struct RomFile {
	data: Arc<RwSpinLock<RomFileInner>>,
}

impl VfsNode for RomFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(RomFileInterface::new(self.data.clone())))
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		Ok(self.data.read().attr)
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
		let attr = FileAttr {
			st_size: length.try_into().unwrap(),
			st_mode: mode | AccessPermission::S_IFREG,
			st_atime: microseconds / 1_000_000,
			st_atime_nsec: (microseconds % 1_000_000) * 1000,
			st_mtime: microseconds / 1_000_000,
			st_mtime_nsec: (microseconds % 1_000_000) * 1000,
			st_ctime: microseconds / 1_000_000,
			st_ctime_nsec: (microseconds % 1_000_000) * 1000,
			..Default::default()
		};

		Self {
			data: unsafe { Arc::new(RwSpinLock::new(RomFileInner::new(ptr, length, attr))) },
		}
	}
}

#[derive(Debug, Clone)]
pub(crate) struct RamFile {
	data: Arc<RwSpinLock<RamFileInner>>,
}

impl VfsNode for RamFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(RamFileInterface::new(self.data.clone())))
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		Ok(self.data.read().attr)
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
		let attr = FileAttr {
			st_mode: mode | AccessPermission::S_IFREG,
			st_atime: microseconds / 1_000_000,
			st_atime_nsec: (microseconds % 1_000_000) * 1000,
			st_mtime: microseconds / 1_000_000,
			st_mtime_nsec: (microseconds % 1_000_000) * 1000,
			st_ctime: microseconds / 1_000_000,
			st_ctime_nsec: (microseconds % 1_000_000) * 1000,
			..Default::default()
		};

		Self {
			data: Arc::new(RwSpinLock::new(RamFileInner::new(attr))),
		}
	}
}

#[derive(Debug)]
pub(crate) struct MemDirectory {
	inner: Arc<
		RwSpinLock<BTreeMap<String, Box<dyn VfsNode + core::marker::Send + core::marker::Sync>>>,
	>,
	attr: FileAttr,
}

impl MemDirectory {
	pub fn new(mode: AccessPermission) -> Self {
		let microseconds = arch::kernel::systemtime::now_micros();

		Self {
			inner: Arc::new(RwSpinLock::new(BTreeMap::new())),
			attr: FileAttr {
				st_mode: mode | AccessPermission::S_IFDIR,
				st_atime: microseconds / 1_000_000,
				st_atime_nsec: (microseconds % 1_000_000) * 1000,
				st_mtime: microseconds / 1_000_000,
				st_mtime_nsec: (microseconds % 1_000_000) * 1000,
				st_ctime: microseconds / 1_000_000,
				st_ctime_nsec: (microseconds % 1_000_000) * 1000,
				..Default::default()
			},
		}
	}
}

impl VfsNode for MemDirectory {
	fn get_kind(&self) -> NodeKind {
		NodeKind::Directory
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		Ok(self.attr)
	}

	fn traverse_mkdir(
		&self,
		components: &mut Vec<&str>,
		mode: AccessPermission,
	) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.read().get(&node_name) {
				return directory.traverse_mkdir(components, mode);
			}

			if components.is_empty() {
				self.inner
					.write()
					.insert(node_name, Box::new(MemDirectory::new(mode)));
				return Ok(());
			}
		}

		Err(IoError::EBADF)
	}

	fn traverse_rmdir(&self, components: &mut Vec<&str>) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				let mut guard = self.inner.write();

				let obj = guard.remove(&node_name).ok_or(IoError::ENOENT)?;
				if obj.get_kind() == NodeKind::Directory {
					return Ok(());
				} else {
					guard.insert(node_name, obj);
					return Err(IoError::ENOTDIR);
				}
			} else if let Some(directory) = self.inner.read().get(&node_name) {
				return directory.traverse_rmdir(components);
			}
		}

		Err(IoError::EBADF)
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				let mut guard = self.inner.write();

				let obj = guard.remove(&node_name).ok_or(IoError::ENOENT)?;
				if obj.get_kind() == NodeKind::File {
					return Ok(());
				} else {
					guard.insert(node_name, obj);
					return Err(IoError::ENOENT);
				}
			} else if let Some(directory) = self.inner.read().get(&node_name) {
				return directory.traverse_unlink(components);
			}
		}

		Err(IoError::EBADF)
	}

	fn traverse_readdir(&self, components: &mut Vec<&str>) -> Result<Vec<DirectoryEntry>, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.read().get(&node_name) {
				directory.traverse_readdir(components)
			} else {
				Err(IoError::EBADF)
			}
		} else {
			let mut entries: Vec<DirectoryEntry> = Vec::new();
			for name in self.inner.read().keys() {
				entries.push(DirectoryEntry::new(name.to_string()));
			}

			Ok(entries)
		}
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				if let Some(node) = self.inner.read().get(&node_name) {
					return node.get_file_attributes();
				}
			}

			if let Some(directory) = self.inner.read().get(&node_name) {
				directory.traverse_lstat(components)
			} else {
				Err(IoError::EBADF)
			}
		} else {
			Err(IoError::ENOSYS)
		}
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				if let Some(node) = self.inner.read().get(&node_name) {
					return node.get_file_attributes();
				}
			}

			if let Some(directory) = self.inner.read().get(&node_name) {
				directory.traverse_stat(components)
			} else {
				Err(IoError::EBADF)
			}
		} else {
			Err(IoError::ENOSYS)
		}
	}

	fn traverse_mount(
		&self,
		components: &mut Vec<&str>,
		obj: Box<dyn VfsNode + core::marker::Send + core::marker::Sync>,
	) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.read().get(&node_name) {
				return directory.traverse_mount(components, obj);
			}

			if components.is_empty() {
				self.inner.write().insert(node_name, obj);
				return Ok(());
			}
		}

		Err(IoError::EBADF)
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
		mode: AccessPermission,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				let mut guard = self.inner.write();
				if opt.contains(OpenOption::O_CREAT) || opt.contains(OpenOption::O_CREAT) {
					if guard.get(&node_name).is_some() {
						return Err(IoError::EEXIST);
					} else {
						let file = Box::new(RamFile::new(mode));
						guard.insert(node_name, file.clone());
						return Ok(Arc::new(RamFileInterface::new(file.data.clone())));
					}
				} else if let Some(file) = guard.get(&node_name) {
					if file.get_kind() == NodeKind::File {
						return file.get_object();
					} else {
						return Err(IoError::ENOENT);
					}
				} else {
					return Err(IoError::ENOENT);
				}
			}

			if let Some(directory) = self.inner.read().get(&node_name) {
				return directory.traverse_open(components, opt, mode);
			}
		}

		Err(IoError::ENOENT)
	}

	fn traverse_create_file(
		&self,
		components: &mut Vec<&str>,
		ptr: *const u8,
		length: usize,
		mode: AccessPermission,
	) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let name = String::from(component);

			if components.is_empty() {
				let file = unsafe { RomFile::new(ptr, length, mode) };
				self.inner.write().insert(name.to_string(), Box::new(file));
				return Ok(());
			}

			if let Some(directory) = self.inner.read().get(&name) {
				return directory.traverse_create_file(components, ptr, length, mode);
			}
		}

		Err(IoError::ENOENT)
	}
}
