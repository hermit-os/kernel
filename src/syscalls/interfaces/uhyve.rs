// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;
use crate::arch::mm::paging;
use crate::syscalls::interfaces::SyscallInterface;
#[cfg(feature = "newlib")]
use crate::syscalls::lwip::sys_lwip_get_errno;
#[cfg(feature = "newlib")]
use crate::syscalls::{LWIP_FD_BIT, LWIP_LOCK};
use core::{mem, ptr, slice};

#[cfg(target_arch = "x86_64")]
use x86::io::*;

const UHYVE_PORT_WRITE: u16 = 0x400;
const UHYVE_PORT_OPEN: u16 = 0x440;
const UHYVE_PORT_CLOSE: u16 = 0x480;
const UHYVE_PORT_READ: u16 = 0x500;
const UHYVE_PORT_EXIT: u16 = 0x540;
const UHYVE_PORT_LSEEK: u16 = 0x580;
const UHYVE_PORT_CMDSIZE: u16 = 0x740;
const UHYVE_PORT_CMDVAL: u16 = 0x780;
const UHYVE_PORT_UNLINK: u16 = 0x840;

#[cfg(feature = "newlib")]
extern "C" {
	fn lwip_write(fd: i32, buf: *const u8, len: usize) -> i32;
	fn lwip_read(fd: i32, buf: *mut u8, len: usize) -> i32;
}

/// forward a request to the hypervisor uhyve
#[inline]
fn uhyve_send<T>(port: u16, data: &mut T) {
	let ptr = data as *mut T;
	let physical_address = paging::virtual_to_physical(ptr as usize);

	#[cfg(target_arch = "x86_64")]
	unsafe {
		outl(port, physical_address as u32);
	}
}

const MAX_ARGC_ENVC: usize = 128;

#[repr(C, packed)]
struct SysCmdsize {
	argc: i32,
	argsz: [i32; MAX_ARGC_ENVC],
	envc: i32,
	envsz: [i32; MAX_ARGC_ENVC],
}

impl SysCmdsize {
	fn new() -> SysCmdsize {
		SysCmdsize {
			argc: 0,
			argsz: [0; MAX_ARGC_ENVC],
			envc: 0,
			envsz: [0; MAX_ARGC_ENVC],
		}
	}
}

#[repr(C, packed)]
struct SysCmdval {
	argv: *const u8,
	envp: *const u8,
}

impl SysCmdval {
	fn new(argv: *const u8, envp: *const u8) -> SysCmdval {
		SysCmdval {
			argv: paging::virtual_to_physical(argv as usize) as *const u8,
			envp: paging::virtual_to_physical(envp as usize) as *const u8,
		}
	}
}

#[repr(C, packed)]
struct SysExit {
	arg: i32,
}

impl SysExit {
	fn new(arg: i32) -> SysExit {
		SysExit { arg }
	}
}

#[repr(C, packed)]
struct SysUnlink {
	name: *const u8,
	ret: i32,
}

impl SysUnlink {
	fn new(name: *const u8) -> SysUnlink {
		SysUnlink {
			name: paging::virtual_to_physical(name as usize) as *const u8,
			ret: -1,
		}
	}
}

#[repr(C, packed)]
struct SysOpen {
	name: *const u8,
	flags: i32,
	mode: i32,
	ret: i32,
}

impl SysOpen {
	fn new(name: *const u8, flags: i32, mode: i32) -> SysOpen {
		SysOpen {
			name: paging::virtual_to_physical(name as usize) as *const u8,
			flags,
			mode,
			ret: -1,
		}
	}
}

#[repr(C, packed)]
struct SysClose {
	fd: i32,
	ret: i32,
}

impl SysClose {
	fn new(fd: i32) -> SysClose {
		SysClose { fd: fd, ret: -1 }
	}
}

#[repr(C, packed)]
struct SysRead {
	fd: i32,
	buf: *const u8,
	len: usize,
	ret: isize,
}

impl SysRead {
	fn new(fd: i32, buf: *const u8, len: usize) -> SysRead {
		SysRead {
			fd,
			buf,
			len,
			ret: -1,
		}
	}
}

#[repr(C, packed)]
struct SysWrite {
	fd: i32,
	buf: *const u8,
	len: usize,
}

impl SysWrite {
	fn new(fd: i32, buf: *const u8, len: usize) -> SysWrite {
		SysWrite { fd, buf, len }
	}
}

#[repr(C, packed)]
struct SysLseek {
	fd: i32,
	offset: isize,
	whence: i32,
}

impl SysLseek {
	fn new(fd: i32, offset: isize, whence: i32) -> SysLseek {
		SysLseek { fd, offset, whence }
	}
}

pub struct Uhyve;

impl SyscallInterface for Uhyve {
	fn open(&self, name: *const u8, flags: i32, mode: i32) -> i32 {
		let mut sysopen = SysOpen::new(name, flags, mode);
		uhyve_send(UHYVE_PORT_OPEN, &mut sysopen);

		sysopen.ret
	}

	fn unlink(&self, name: *const u8) -> i32 {
		let mut sysunlink = SysUnlink::new(name);
		uhyve_send(UHYVE_PORT_UNLINK, &mut sysunlink);

		sysunlink.ret
	}
	fn close(&self, fd: i32) -> i32 {
		let mut sysclose = SysClose::new(fd);
		uhyve_send(UHYVE_PORT_CLOSE, &mut sysclose);

		sysclose.ret
	}

	#[cfg(not(test))]
	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		// determine the number of arguments and environment variables
		let mut syscmdsize = SysCmdsize::new();
		uhyve_send(UHYVE_PORT_CMDSIZE, &mut syscmdsize);

		// create array to receive all arguments
		let argv_raw = crate::__sys_malloc(
			syscmdsize.argc as usize * mem::size_of::<*const u8>(),
			mem::size_of::<*const u8>(),
		) as *mut *const u8;
		let argv_phy_raw = crate::__sys_malloc(
			syscmdsize.argc as usize * mem::size_of::<*const u8>(),
			mem::size_of::<*const u8>(),
		) as *mut *const u8;
		let argv = unsafe { slice::from_raw_parts_mut(argv_raw, syscmdsize.argc as usize) };
		let argv_phy = unsafe { slice::from_raw_parts_mut(argv_phy_raw, syscmdsize.argc as usize) };
		for i in 0..syscmdsize.argc as usize {
			argv[i] = crate::__sys_malloc(
				syscmdsize.argsz[i] as usize * mem::size_of::<*const u8>(),
				1,
			);
			argv_phy[i] = paging::virtual_to_physical(argv[i] as usize) as *const u8;
		}

		// create array to receive the environment
		let env_raw = crate::__sys_malloc(
			(syscmdsize.envc + 1) as usize * mem::size_of::<*const u8>(),
			mem::size_of::<*const u8>(),
		) as *mut *const u8;
		let env_phy_raw = crate::__sys_malloc(
			(syscmdsize.envc + 1) as usize * mem::size_of::<*const u8>(),
			mem::size_of::<*const u8>(),
		) as *mut *const u8;
		let env = unsafe { slice::from_raw_parts_mut(env_raw, (syscmdsize.envc + 1) as usize) };
		let env_phy =
			unsafe { slice::from_raw_parts_mut(env_phy_raw, (syscmdsize.envc + 1) as usize) };
		for i in 0..syscmdsize.envc as usize {
			env[i] = crate::__sys_malloc(
				syscmdsize.envsz[i] as usize * mem::size_of::<*const u8>(),
				1,
			);
			env_phy[i] = paging::virtual_to_physical(env[i] as usize) as *const u8;
		}
		env[syscmdsize.envc as usize] = ptr::null_mut();
		env_phy[syscmdsize.envc as usize] = ptr::null_mut();

		// ask uhyve for the environment
		let mut syscmdval = SysCmdval::new(argv_phy_raw as *const u8, env_phy_raw as *const u8);
		uhyve_send(UHYVE_PORT_CMDVAL, &mut syscmdval);

		// free temporary array
		crate::__sys_free(
			argv_phy_raw as *mut u8,
			syscmdsize.argc as usize * mem::size_of::<*const u8>(),
			mem::size_of::<*const u8>(),
		);
		crate::__sys_free(
			env_phy_raw as *mut u8,
			(syscmdsize.envc + 1) as usize * mem::size_of::<*const u8>(),
			mem::size_of::<*const u8>(),
		);

		(
			syscmdsize.argc,
			argv_raw as *const *const u8,
			env_raw as *const *const u8,
		)
	}

	fn shutdown(&self, arg: i32) -> ! {
		let mut sysexit = SysExit::new(arg);
		uhyve_send(UHYVE_PORT_EXIT, &mut sysexit);

		loop {
			arch::processor::halt();
		}
	}

	fn read(&self, fd: i32, buf: *mut u8, len: usize) -> isize {
		// do we have an LwIP file descriptor?
		#[cfg(feature = "newlib")]
		{
			if (fd & LWIP_FD_BIT) != 0 {
				// take lock to protect LwIP
				let _guard = LWIP_LOCK.lock();
				let ret;

				unsafe {
					ret = lwip_read(fd & !LWIP_FD_BIT, buf as *mut u8, len);
				}
				if ret < 0 {
					return -sys_lwip_get_errno() as isize;
				}

				return ret as isize;
			}
		}

		let mut sysread = SysRead::new(fd, buf, len);
		uhyve_send(UHYVE_PORT_READ, &mut sysread);

		sysread.ret
	}

	fn write(&self, fd: i32, buf: *const u8, len: usize) -> isize {
		// do we have an LwIP file descriptor?
		#[cfg(feature = "newlib")]
		{
			if (fd & LWIP_FD_BIT) != 0 {
				// take lock to protect LwIP
				let _guard = LWIP_LOCK.lock();
				let ret;

				unsafe {
					ret = lwip_write(fd & !LWIP_FD_BIT, buf as *const u8, len);
				}
				if ret < 0 {
					return -sys_lwip_get_errno() as isize;
				}

				return ret as isize;
			}
		}

		let mut syswrite = SysWrite::new(fd, buf, len);
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		syswrite.len as isize
	}

	fn lseek(&self, fd: i32, offset: isize, whence: i32) -> isize {
		let mut syslseek = SysLseek::new(fd, offset, whence);
		uhyve_send(UHYVE_PORT_LSEEK, &mut syslseek);

		syslseek.offset
	}
}
