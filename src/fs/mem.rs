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
use core::ops::{Deref, DerefMut};
use core::slice;

use hermit_sync::{RwSpinLock, SpinMutex};

use crate::fd::{AccessPermission, IoError, ObjectInterface, OpenOption};
use crate::fs::{DirectoryEntry, FileAttr, NodeKind, VfsNode};

#[derive(Debug)]
struct RomFileInterface {
	/// Position within the file
	pos: SpinMutex<usize>,
	/// File content
	data: Arc<RwSpinLock<&'static [u8]>>,
}

impl ObjectInterface for RomFileInterface {
	fn read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
		let vec = self.data.read();
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
	pub fn new(data: Arc<RwSpinLock<&'static [u8]>>) -> Self {
		Self {
			pos: SpinMutex::new(0),
			data,
		}
	}

	pub fn len(&self) -> usize {
		let guard = self.data.read();
		guard.len()
	}
}

impl Clone for RomFileInterface {
	fn clone(&self) -> Self {
		Self {
			pos: SpinMutex::new(*self.pos.lock()),
			data: self.data.clone(),
		}
	}
}

#[derive(Debug)]
pub struct RamFileInterface {
	/// Position within the file
	pos: SpinMutex<usize>,
	/// File content
	data: Arc<RwSpinLock<Vec<u8>>>,
}

impl ObjectInterface for RamFileInterface {
	fn read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
		let guard = self.data.read();
		let vec = guard.deref();
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

	fn write(&self, buf: &[u8]) -> Result<usize, IoError> {
		let mut guard = self.data.write();
		let vec = guard.deref_mut();
		let mut pos_guard = self.pos.lock();
		let pos = *pos_guard;

		if pos + buf.len() > vec.len() {
			vec.resize(pos + buf.len(), 0);
		}

		vec[pos..pos + buf.len()].clone_from_slice(buf);
		*pos_guard = pos + buf.len();

		Ok(buf.len())
	}
}

impl RamFileInterface {
	pub fn new(data: Arc<RwSpinLock<Vec<u8>>>) -> Self {
		Self {
			pos: SpinMutex::new(0),
			data,
		}
	}

	pub fn len(&self) -> usize {
		let guard = self.data.read();
		let vec: &Vec<u8> = guard.deref();
		vec.len()
	}
}

impl Clone for RamFileInterface {
	fn clone(&self) -> Self {
		Self {
			pos: SpinMutex::new(*self.pos.lock()),
			data: self.data.clone(),
		}
	}
}

#[derive(Debug, Clone)]
pub(crate) struct RomFile {
	data: Arc<RwSpinLock<&'static [u8]>>,
	attr: FileAttr,
}

impl VfsNode for RomFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(RomFileInterface::new(self.data.clone())))
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		let mut attr = self.attr;
		attr.st_size = self.data.read().len().try_into().unwrap();
		Ok(attr)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			Ok(self.attr)
		} else {
			Err(IoError::EBADF)
		}
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			Ok(self.attr)
		} else {
			Err(IoError::EBADF)
		}
	}
}

impl RomFile {
	pub unsafe fn new(ptr: *const u8, length: usize, mode: AccessPermission) -> Self {
		Self {
			data: Arc::new(RwSpinLock::new(unsafe {
				slice::from_raw_parts(ptr, length)
			})),
			attr: FileAttr {
				st_mode: mode,
				..Default::default()
			},
		}
	}
}

#[derive(Debug, Clone)]
pub(crate) struct RamFile {
	data: Arc<RwSpinLock<Vec<u8>>>,
	attr: FileAttr,
}

impl VfsNode for RamFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(Arc::new(RamFileInterface::new(self.data.clone())))
	}

	fn get_file_attributes(&self) -> Result<FileAttr, IoError> {
		let mut attr = self.attr;
		attr.st_size = self.data.read().len().try_into().unwrap();
		Ok(attr)
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			Ok(self.attr)
		} else {
			Err(IoError::EBADF)
		}
	}

	fn traverse_stat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if components.is_empty() {
			Ok(self.attr)
		} else {
			Err(IoError::EBADF)
		}
	}
}

impl RamFile {
	pub fn new(mode: AccessPermission) -> Self {
		Self {
			data: Arc::new(RwSpinLock::new(Vec::new())),
			attr: FileAttr {
				st_mode: mode,
				..Default::default()
			},
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
		Self {
			inner: Arc::new(RwSpinLock::new(BTreeMap::new())),
			attr: FileAttr {
				st_mode: mode,
				..Default::default()
			},
		}
	}

	pub fn create_file(
		&self,
		name: &str,
		ptr: *const u8,
		length: usize,
		mode: AccessPermission,
	) -> Result<(), IoError> {
		let name = name.trim();
		if name.find('/').is_none() {
			let file = unsafe { RomFile::new(ptr, length, mode) };
			self.inner.write().insert(name.to_string(), Box::new(file));
			Ok(())
		} else {
			Err(IoError::EBADF)
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

			if let Some(directory) = self.inner.read().get(&node_name) {
				return directory.traverse_rmdir(components);
			}

			if components.is_empty() {
				let mut guard = self.inner.write();

				let obj = guard.remove(&node_name).ok_or(IoError::ENOENT)?;
				if obj.get_kind() == NodeKind::Directory {
					return Ok(());
				} else {
					guard.insert(node_name, obj);
					return Err(IoError::ENOTDIR);
				}
			}
		}

		Err(IoError::EBADF)
	}

	fn traverse_unlink(&self, components: &mut Vec<&str>) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.read().get(&node_name) {
				return directory.traverse_unlink(components);
			}

			if components.is_empty() {
				let mut guard = self.inner.write();

				let obj = guard.remove(&node_name).ok_or(IoError::ENOENT)?;
				if obj.get_kind() == NodeKind::Directory {
					guard.insert(node_name, obj);
					return Err(IoError::EISDIR);
				} else {
					return Ok(());
				}
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
				entries.push(DirectoryEntry::new(name.as_bytes()));
			}

			Ok(entries)
		}
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				if let Some(node) = self.inner.read().get(&node_name) {
					node.get_file_attributes()?;
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
					node.get_file_attributes()?;
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
}
