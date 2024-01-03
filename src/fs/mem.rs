// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Implements basic functions to realize a simple in-memory file system

#![allow(dead_code)]

use alloc::alloc::{alloc_zeroed, Layout};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::slice;
use core::sync::atomic::{AtomicUsize, Ordering};

use hermit_sync::{RwSpinLock, SpinMutex};

use crate::fd::{DirectoryEntry, Dirent, IoError, ObjectInterface, OpenOption};
use crate::fs::{FileAttr, NodeKind, VfsNode};

#[derive(Debug)]
struct RomFileInner {
	/// Position within the file
	pos: SpinMutex<usize>,
	/// File content
	data: Arc<RwSpinLock<&'static [u8]>>,
}

impl ObjectInterface for RomFileInner {
	fn close(&self) {
		*self.pos.lock() = 0;
	}

	fn read(&self, buf: &mut [u8]) -> Result<isize, IoError> {
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

		Ok(len.try_into().unwrap())
	}
}

impl RomFileInner {
	pub unsafe fn new(addr: *const u8, len: usize) -> Self {
		Self {
			pos: SpinMutex::new(0),
			data: Arc::new(RwSpinLock::new(unsafe { slice::from_raw_parts(addr, len) })),
		}
	}

	pub fn len(&self) -> usize {
		let guard = self.data.read();
		guard.len()
	}
}

impl Clone for RomFileInner {
	fn clone(&self) -> Self {
		RomFileInner {
			pos: SpinMutex::new(0),
			data: self.data.clone(),
		}
	}
}

#[derive(Debug)]
pub struct RamFileInner {
	/// Position within the file
	pos: SpinMutex<usize>,
	/// File content
	data: Arc<RwSpinLock<Vec<u8>>>,
}

impl ObjectInterface for RamFileInner {
	fn close(&self) {
		*self.pos.lock() = 0;
	}

	fn read(&self, buf: &mut [u8]) -> Result<isize, IoError> {
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

		Ok(len.try_into().unwrap())
	}

	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		let mut guard = self.data.write();
		let vec = guard.deref_mut();
		let mut pos_guard = self.pos.lock();
		let pos = *pos_guard;

		if pos + buf.len() > vec.len() {
			vec.resize(pos + buf.len(), 0);
		}

		vec[pos..pos + buf.len()].clone_from_slice(buf);
		*pos_guard = pos + buf.len();

		Ok(buf.len().try_into().unwrap())
	}
}

impl RamFileInner {
	pub fn new() -> Self {
		Self {
			pos: SpinMutex::new(0),
			data: Arc::new(RwSpinLock::new(Vec::new())),
		}
	}

	pub fn len(&self) -> usize {
		let guard = self.data.read();
		let vec: &Vec<u8> = guard.deref();
		vec.len()
	}
}

impl Clone for RamFileInner {
	fn clone(&self) -> Self {
		RamFileInner {
			pos: SpinMutex::new(0),
			data: self.data.clone(),
		}
	}
}

#[derive(Debug, Clone)]
pub(crate) struct RomFile(Arc<RomFileInner>);

impl VfsNode for RomFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(self.0.clone())
	}
}

impl RomFile {
	pub unsafe fn new(ptr: *const u8, length: usize) -> Self {
		Self(Arc::new(RomFileInner::new(ptr, length)))
	}
}

#[derive(Debug, Clone)]
pub(crate) struct RamFile(Arc<RamFileInner>);

impl VfsNode for RamFile {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> Result<Arc<dyn ObjectInterface>, IoError> {
		Ok(self.0.clone())
	}
}

impl RamFile {
	pub fn new() -> Self {
		Self(Arc::new(RamFileInner::new()))
	}
}

#[derive(Debug)]
struct MemDirectoryInner(
	pub  Arc<
		RwSpinLock<BTreeMap<String, Box<dyn VfsNode + core::marker::Send + core::marker::Sync>>>,
	>,
	AtomicUsize,
);

impl MemDirectoryInner {
	pub fn new() -> Self {
		Self(
			Arc::new(RwSpinLock::new(BTreeMap::new())),
			AtomicUsize::new(0),
		)
	}
}

impl ObjectInterface for MemDirectoryInner {
	fn readdir(&self) -> DirectoryEntry {
		let pos = self.1.fetch_add(1, Ordering::SeqCst);

		if pos == 0 {
			let name = ".";
			let name_len = name.len();

			let len = core::mem::size_of::<Dirent>() + name_len + 1;
			let layout = Layout::from_size_align(len, core::mem::align_of::<Dirent>())
				.unwrap()
				.pad_to_align();

			let raw = unsafe {
				let raw = alloc_zeroed(layout) as *mut Dirent;
				(*raw).d_namelen = name_len.try_into().unwrap();
				core::ptr::copy_nonoverlapping(
					name.as_ptr(),
					&mut (*raw).d_name as *mut u8,
					name_len,
				);

				raw
			};

			DirectoryEntry::Valid(raw)
		} else if pos == 1 {
			let name = "..";
			let name_len = name.len();

			let len = core::mem::size_of::<Dirent>() + name_len + 1;
			let layout = Layout::from_size_align(len, core::mem::align_of::<Dirent>())
				.unwrap()
				.pad_to_align();

			let raw = unsafe {
				let raw = alloc_zeroed(layout) as *mut Dirent;
				(*raw).d_namelen = name_len.try_into().unwrap();
				core::ptr::copy_nonoverlapping(
					name.as_ptr(),
					&mut (*raw).d_name as *mut u8,
					name_len,
				);

				raw
			};

			DirectoryEntry::Valid(raw)
		} else {
			let keys: Vec<_> = self.0.read().keys().cloned().collect();

			if keys.len() > pos - 2 {
				let name_len = keys[pos - 2].len();

				let len = core::mem::size_of::<Dirent>() + name_len + 1;
				let layout = Layout::from_size_align(len, core::mem::align_of::<Dirent>())
					.unwrap()
					.pad_to_align();

				let raw = unsafe {
					let raw = alloc_zeroed(layout) as *mut Dirent;
					(*raw).d_namelen = name_len.try_into().unwrap();
					core::ptr::copy_nonoverlapping(
						keys[pos - 2].as_ptr(),
						&mut (*raw).d_name as *mut u8,
						name_len,
					);

					raw
				};

				DirectoryEntry::Valid(raw)
			} else {
				DirectoryEntry::Valid(core::ptr::null())
			}
		}
	}
}

impl Clone for MemDirectoryInner {
	fn clone(&self) -> Self {
		Self(
			self.0.clone(),
			AtomicUsize::new(self.1.load(Ordering::Relaxed)),
		)
	}
}

#[derive(Debug)]
pub(crate) struct MemDirectory {
	inner: Arc<MemDirectoryInner>,
}

impl MemDirectory {
	pub fn new() -> Self {
		Self {
			inner: Arc::new(MemDirectoryInner::new()),
		}
	}

	pub fn create_file(&self, name: &str, ptr: *const u8, length: usize) -> Result<(), IoError> {
		let name = name.trim();
		if name.find('/').is_none() {
			let file = unsafe { RomFile::new(ptr, length) };
			self.inner
				.0
				.write()
				.insert(name.to_string(), Box::new(file));
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

	fn traverse_mkdir(&self, components: &mut Vec<&str>, mode: u32) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.0.read().get(&node_name) {
				return directory.traverse_mkdir(components, mode);
			}

			if components.is_empty() {
				self.inner
					.0
					.write()
					.insert(node_name, Box::new(MemDirectory::new()));
				return Ok(());
			}
		}

		Err(IoError::EBADF)
	}

	fn traverse_rmdir(&self, components: &mut Vec<&str>) -> Result<(), IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.0.read().get(&node_name) {
				return directory.traverse_rmdir(components);
			}

			if components.is_empty() {
				let mut guard = self.inner.0.write();

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

			if let Some(directory) = self.inner.0.read().get(&node_name) {
				return directory.traverse_unlink(components);
			}

			if components.is_empty() {
				let mut guard = self.inner.0.write();

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

	fn traverse_opendir(
		&self,
		components: &mut Vec<&str>,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.0.read().get(&node_name) {
				directory.traverse_opendir(components)
			} else {
				Err(IoError::EBADF)
			}
		} else {
			Ok(self.inner.clone())
		}
	}

	fn traverse_lstat(&self, components: &mut Vec<&str>) -> Result<FileAttr, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if let Some(directory) = self.inner.0.read().get(&node_name) {
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

			if let Some(directory) = self.inner.0.read().get(&node_name) {
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

			if let Some(directory) = self.inner.0.read().get(&node_name) {
				return directory.traverse_mount(components, obj);
			}

			if components.is_empty() {
				self.inner.0.write().insert(node_name, obj);
				return Ok(());
			}
		}

		Err(IoError::EBADF)
	}

	fn traverse_open(
		&self,
		components: &mut Vec<&str>,
		opt: OpenOption,
	) -> Result<Arc<dyn ObjectInterface>, IoError> {
		if let Some(component) = components.pop() {
			let node_name = String::from(component);

			if components.is_empty() {
				let mut guard = self.inner.0.write();
				if opt.contains(OpenOption::O_CREAT) || opt.contains(OpenOption::O_CREAT) {
					if guard.get(&node_name).is_some() {
						return Err(IoError::EEXIST);
					} else {
						let file = Box::new(RamFile::new());
						guard.insert(node_name, file.clone());
						return Ok(file.0.clone());
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

			if let Some(directory) = self.inner.0.read().get(&node_name) {
				return directory.traverse_open(components, opt);
			}
		}

		Err(IoError::ENOENT)
	}
}
