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
use core::ops::Deref;
use core::slice;
use core::sync::atomic::{AtomicUsize, Ordering};

use hermit_sync::RwSpinLock;

use crate::fd::{DirectoryEntry, Dirent, IoError, ObjectInterface, OpenOption};
use crate::fs::{FileAttr, NodeKind, VfsNode};

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
			let file = unsafe { MemFile::from_raw_parts(ptr, length) };
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
	/// Returns the node type
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

			if let Some(directory) = self.inner.0.read().get(&node_name) {
				return directory.traverse_open(components, opt);
			}

			/*if components.is_empty() {
				self.children.insert(node_name, obj);
				return Ok(());
			}*/
		}

		Err(IoError::EBADF)
	}
}

#[derive(Debug)]
pub struct RomHandle {
	/// Position within the file
	pos: AtomicUsize,
	/// File content
	data: Arc<RwSpinLock<&'static [u8]>>,
}

impl RomHandle {
	pub unsafe fn new(addr: *const u8, len: usize) -> Self {
		RomHandle {
			pos: AtomicUsize::new(0),
			data: Arc::new(RwSpinLock::new(unsafe { slice::from_raw_parts(addr, len) })),
		}
	}

	pub fn len(&self) -> usize {
		let guard = self.data.read();
		guard.len()
	}
}

impl Clone for RomHandle {
	fn clone(&self) -> Self {
		RomHandle {
			pos: AtomicUsize::new(self.pos.load(Ordering::Relaxed)),
			data: self.data.clone(),
		}
	}
}

#[derive(Debug)]
pub struct RamHandle {
	/// Position within the file
	pos: AtomicUsize,
	/// File content
	data: Arc<RwSpinLock<Vec<u8>>>,
}

impl RamHandle {
	pub fn new() -> Self {
		RamHandle {
			pos: AtomicUsize::new(0),
			data: Arc::new(RwSpinLock::new(Vec::new())),
		}
	}

	pub fn len(&self) -> usize {
		let guard = self.data.read();
		let vec: &Vec<u8> = guard.deref();
		vec.len()
	}
}

impl Clone for RamHandle {
	fn clone(&self) -> Self {
		RamHandle {
			pos: AtomicUsize::new(self.pos.load(Ordering::Relaxed)),
			data: self.data.clone(),
		}
	}
}

/// Enumeration of possible methods to seek within an I/O object.
#[derive(Debug, Clone)]
enum DataHandle {
	Ram(RamHandle),
	Rom(RomHandle),
}

#[derive(Debug)]
pub(crate) struct MemFile {
	/// File content
	data: DataHandle,
}

impl MemFile {
	pub fn new() -> Self {
		Self {
			data: DataHandle::Ram(RamHandle::new()),
		}
	}

	pub unsafe fn from_raw_parts(ptr: *const u8, length: usize) -> Self {
		Self {
			data: unsafe { DataHandle::Rom(RomHandle::new(ptr, length)) },
		}
	}
}

impl VfsNode for MemFile {
	/// Returns the node type
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}
}
