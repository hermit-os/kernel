#![no_std]
#![no_main]
#![test_runner(common::test_case_runner)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

mod common;

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::CStr;

use hermit::syscalls::{
	Dirent64, PosixDent, sys_close, sys_getdents64, sys_lseek, sys_mkdir, sys_open, sys_opendir,
	sys_posix_getdents,
};

const EINVAL: i32 = 22;
const ENOTDIR: i32 = 20;

const O_WRONLY: i32 = 0o1;
const O_CREAT: i32 = 0o100;
const O_DIRECTORY: i32 = 0o200_000;

#[repr(align(8))]
struct Buf([u8; 1024]);

unsafe fn parse_dirent64(buf: &Buf, len: usize) -> Vec<(u64, u16, String)> {
	let mut entries = Vec::new();
	let mut offset = 0;
	while offset < len {
		let dirent = unsafe { &*buf.0.as_ptr().add(offset).cast::<Dirent64>() };
		let name = unsafe { CStr::from_ptr((&raw const dirent.d_name).cast()) };
		entries.push((
			dirent.d_ino,
			dirent.d_reclen,
			String::from(name.to_str().unwrap()),
		));
		offset += usize::from(dirent.d_reclen);
	}
	assert_eq!(offset, len);
	entries
}

unsafe fn parse_posix_dent(buf: &Buf, len: usize) -> Vec<(u64, u16, String)> {
	let mut entries = Vec::new();
	let mut offset = 0;
	while offset < len {
		let dirent = unsafe { &*buf.0.as_ptr().add(offset).cast::<PosixDent>() };
		let name = unsafe { CStr::from_ptr((&raw const dirent.d_name).cast()) };
		entries.push((
			dirent.d_ino,
			dirent.d_reclen,
			String::from(name.to_str().unwrap()),
		));
		offset += usize::from(dirent.d_reclen);
	}
	assert_eq!(offset, len);
	entries
}

fn check_dir_fd(fd: i32) {
	let mut buf = Buf([0; 1024]);

	// getdents64: Dirent64 has a 19-byte header, entries are 8-byte aligned.
	let len = unsafe { sys_getdents64(fd, buf.0.as_mut_ptr().cast(), buf.0.len()) };
	assert!(len > 0);
	let entries = unsafe { parse_dirent64(&buf, len.try_into().unwrap()) };
	assert_eq!(entries.len(), 3);
	assert_eq!(entries[0], (1, 24, String::from("a")));
	assert_eq!(entries[1], (1, 24, String::from("bb")));
	assert_eq!(entries[2], (1, 24, String::from("ccc")));

	// End of directory
	let len = unsafe { sys_getdents64(fd, buf.0.as_mut_ptr().cast(), buf.0.len()) };
	assert_eq!(len, 0);

	// posix_getdents: PosixDent has an 11-byte header, entries are 8-byte aligned.
	assert_eq!(sys_lseek(fd, 0, 0), 0);
	let len = unsafe { sys_posix_getdents(fd, buf.0.as_mut_ptr().cast(), buf.0.len(), 0) };
	assert!(len > 0);
	let entries = unsafe { parse_posix_dent(&buf, len.try_into().unwrap()) };
	assert_eq!(entries.len(), 3);
	assert_eq!(entries[0], (1, 16, String::from("a")));
	assert_eq!(entries[1], (1, 16, String::from("bb")));
	assert_eq!(entries[2], (1, 16, String::from("ccc")));

	// End of directory
	let len = unsafe { sys_posix_getdents(fd, buf.0.as_mut_ptr().cast(), buf.0.len(), 0) };
	assert_eq!(len, 0);

	// Partial reads: a buffer with space for exactly one entry
	assert_eq!(sys_lseek(fd, 0, 0), 0);
	let len = unsafe { sys_posix_getdents(fd, buf.0.as_mut_ptr().cast(), 16, 0) };
	assert_eq!(len, 16);
	let entries = unsafe { parse_posix_dent(&buf, 16) };
	assert_eq!(entries[0].2, "a");
	let len = unsafe { sys_posix_getdents(fd, buf.0.as_mut_ptr().cast(), buf.0.len(), 0) };
	assert_eq!(len, 32);
	let entries = unsafe { parse_posix_dent(&buf, 32) };
	assert_eq!(entries[0].2, "bb");
	assert_eq!(entries[1].2, "ccc");

	// No flags are supported
	let len = unsafe { sys_posix_getdents(fd, buf.0.as_mut_ptr().cast(), buf.0.len(), 1) };
	assert_eq!(len, -isize::try_from(EINVAL).unwrap());

	// A buffer too small for even one entry
	assert_eq!(sys_lseek(fd, 0, 0), 0);
	let len = unsafe { sys_posix_getdents(fd, buf.0.as_mut_ptr().cast(), 8, 0) };
	assert_eq!(len, -isize::try_from(EINVAL).unwrap());

	assert_eq!(sys_close(fd), 0);
}

#[test_case]
fn getdents() {
	assert_eq!(unsafe { sys_mkdir(c"/tmp/gd".as_ptr(), 0o755) }, 0);
	for name in [c"/tmp/gd/a", c"/tmp/gd/bb", c"/tmp/gd/ccc"] {
		let fd = unsafe { sys_open(name.as_ptr(), O_WRONLY | O_CREAT, 0o644) };
		assert!(fd >= 0);
		assert_eq!(sys_close(fd), 0);
	}

	// Directory opened with opendir
	let fd = unsafe { sys_opendir(c"/tmp/gd".as_ptr()) };
	assert!(fd >= 0);
	check_dir_fd(fd);

	// Directory opened with O_DIRECTORY
	let fd = unsafe { sys_open(c"/tmp/gd".as_ptr(), O_DIRECTORY, 0) };
	assert!(fd >= 0);
	check_dir_fd(fd);

	// A non-directory file descriptor
	let mut buf = Buf([0; 1024]);
	let fd = unsafe { sys_open(c"/tmp/gd/a".as_ptr(), O_WRONLY, 0) };
	assert!(fd >= 0);
	let len = unsafe { sys_posix_getdents(fd, buf.0.as_mut_ptr().cast(), buf.0.len(), 0) };
	assert_eq!(len, -isize::try_from(ENOTDIR).unwrap());
	assert_eq!(sys_close(fd), 0);
}

#[unsafe(no_mangle)]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	test_main();
	common::exit(false)
}
