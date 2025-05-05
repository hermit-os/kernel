//! A module for custom IOCTL objects

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Debug;

use bitfield_struct::bitfield;

use crate::fd::{AccessPermission, ObjectInterface};
use crate::fs::{NodeKind, VfsNode};
use crate::io;

/// Encoding for an IOCTL command, as done in the Linux Kernel.
///
/// See [relevant kernel header](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/include/uapi/asm-generic/ioctl.h?h=v6.15) for reference.
///
/// The goal of this interface is to easily support linux applications that communicate via IOCTL,
/// so linux compatibility is an intended and explicit goal here.
#[bitfield(u32)]
pub struct IoCtlCall {
	call_nr: u8,

	call_type: u8,

	#[bits(2, from = IoCtlDirection::from_bits_truncate, default = IoCtlDirection::empty())]
	call_dir: IoCtlDirection,

	#[bits(14)]
	call_size: u16,
}

bitflags! {
	#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
	pub struct IoCtlDirection: u8 {
		const IOC_WRITE = 1;
		const IOC_READ = 2;
	}
}

impl IoCtlDirection {
	// Required for IoCtlCall
	const fn into_bits(self) -> u8 {
		self.bits()
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

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::{IoCtlCall, IoCtlDirection};

	#[test]
	fn ioctl_call_correctly_written() {
		let call_nr = 0x12u8;
		let call_type = 0x78u8;
		let call_dir = IoCtlDirection::IOC_WRITE;
		let call_size = 0x423u16;

		let ioctl_call_number = (u32::from(call_size) << 18)
			| (u32::from(call_dir.bits()) << 16)
			| (u32::from(call_type) << 8)
			| u32::from(call_nr);

		let call = IoCtlCall::new()
			.with_call_nr(call_nr)
			.with_call_type(call_type)
			.with_call_dir(call_dir)
			.with_call_size(call_size);

		assert_eq!(ioctl_call_number, call.into_bits());
	}
	#[test]
	fn ioctl_call_correctly_parsed() {
		let call_nr = 0x12u8;
		let call_type = 0x78u8;
		let call_dir = IoCtlDirection::IOC_WRITE;
		let call_size = 0x423u16;

		let ioctl_call_number = (u32::from(call_size) << 18)
			| (u32::from(call_dir.bits()) << 16)
			| (u32::from(call_type) << 8)
			| u32::from(call_nr);

		let parsed = IoCtlCall::from_bits(ioctl_call_number);

		assert_eq!(call_nr, parsed.call_nr());
		assert_eq!(call_type, parsed.call_type());
		assert_eq!(call_dir, parsed.call_dir());
		assert_eq!(call_size, parsed.call_size());
	}
}
