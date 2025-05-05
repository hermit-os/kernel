//! A module for custom IOCTL objects

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Debug;

use crate::fd::{AccessPermission, ObjectInterface};
use crate::fs::{NodeKind, VfsNode};
use crate::io;

#[derive(Copy, Clone)]
pub struct IoCtlCall(pub u32);

impl Debug for IoCtlCall {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.debug_struct("IoCtlCall")
			.field("call_nr", &self.call_nr())
			.field("call_type", &self.call_type())
			.field("call_size", &self.call_size())
			.field("call_dir", &self.call_dir())
			.field(".0", &self.0)
			.finish()
	}
}

bitflags! {
	#[derive(Debug, Copy, Clone, Default)]
	pub struct IoCtlDirection: u8 {
		const IOC_WRITE = 1;
		const IOC_READ = 2;
	}
}

impl IoCtlCall {
	pub fn call_nr(&self) -> u8 {
		(self.0 & 0xff) as u8
	}

	pub fn call_type(&self) -> u8 {
		((self.0 >> 8) & 0xff) as u8
	}

	pub fn call_dir(&self) -> IoCtlDirection {
		let dir = (self.0 >> 30) & 0x3;
		IoCtlDirection::from_bits_truncate(dir as u8)
	}

	pub fn call_size(&self) -> u16 {
		((self.0 >> 16) & 0x3fff) as u16
	}
}

#[derive(Debug)]
struct IoCtlNode(Arc<dyn ObjectInterface>);

impl VfsNode for IoCtlNode {
	fn get_kind(&self) -> NodeKind {
		NodeKind::File
	}

	fn get_object(&self) -> io::Result<Arc<dyn ObjectInterface>> {
		Ok(self.0.clone())
	}
}

/// Register a custom object to handle IOCTLS at a given path,
///
/// This call mounts a trivial VfsNode that opens to the provided ioctl_object at the given path.
#[allow(dead_code)]
pub(crate) fn register_ioctl(path: &str, ioctl_object: Arc<dyn ObjectInterface>) {
	assert!(path.starts_with("/"));
	let mut path: Vec<&str> = path.split("/").skip(1).collect();
	assert!(!path.is_empty());

	let fs = super::FILESYSTEM
		.get()
		.expect("Failed to mount ioctl: filesystem is not yet initialized");

	// Create parent directory
	let mut directory: Vec<&str> = path.clone();
	directory.pop().unwrap(); // remove file name
	directory.reverse();

	if !directory.is_empty() {
		let _ = fs
			.root
			.traverse_mkdir(&mut directory, AccessPermission::all()); // ignore possible errors at this step
	}

	// Mount the file
	path.reverse();
	fs.root
		.traverse_mount(&mut path, Box::new(IoCtlNode(ioctl_object)))
		.expect("Failed to mount ioctl: filesystem error");
}
