use alloc::{boxed::Box, vec::Vec};
use core::mem;

#[cfg(target_arch = "x86_64")]
use x86::io::*;

use crate::arch;
use crate::arch::mm::paging;
use crate::arch::mm::{PhysAddr, VirtAddr};
use crate::syscalls::interfaces::SyscallInterface;
#[cfg(feature = "newlib")]
use crate::syscalls::lwip::sys_lwip_get_errno;
#[cfg(feature = "newlib")]
use crate::syscalls::{LWIP_FD_BIT, LWIP_LOCK};

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
#[cfg(target_arch = "x86_64")]
fn uhyve_send<T>(port: u16, data: &mut T) {
	let ptr = VirtAddr(data as *mut _ as u64);
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	unsafe {
		outl(port, physical_address.as_u64() as u32);
	}
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "aarch64")]
fn uhyve_send<T>(port: u16, data: &mut T) {
	use core::arch::asm;

	let ptr = VirtAddr(data as *mut _ as u64);
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	unsafe {
		asm!(
			"str x8, [{port}]",
			port = in(reg) port,
			in("x8") physical_address.as_u64(),
			options(nostack),
		);
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
	argv: PhysAddr,
	envp: PhysAddr,
}

impl SysCmdval {
	fn new(argv: VirtAddr, envp: VirtAddr) -> SysCmdval {
		SysCmdval {
			argv: paging::virtual_to_physical(argv).unwrap(),
			envp: paging::virtual_to_physical(envp).unwrap(),
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
	name: PhysAddr,
	ret: i32,
}

impl SysUnlink {
	fn new(name: VirtAddr) -> SysUnlink {
		SysUnlink {
			name: paging::virtual_to_physical(name).unwrap(),
			ret: -1,
		}
	}
}

#[repr(C, packed)]
struct SysOpen {
	name: PhysAddr,
	flags: i32,
	mode: i32,
	ret: i32,
}

impl SysOpen {
	fn new(name: VirtAddr, flags: i32, mode: i32) -> SysOpen {
		SysOpen {
			name: paging::virtual_to_physical(name).unwrap(),
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
		SysClose { fd, ret: -1 }
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
		let mut sysopen = SysOpen::new(VirtAddr(name as u64), flags, mode);
		uhyve_send(UHYVE_PORT_OPEN, &mut sysopen);

		sysopen.ret
	}

	fn unlink(&self, name: *const u8) -> i32 {
		let mut sysunlink = SysUnlink::new(VirtAddr(name as u64));
		uhyve_send(UHYVE_PORT_UNLINK, &mut sysunlink);

		sysunlink.ret
	}

	fn close(&self, fd: i32) -> i32 {
		let mut sysclose = SysClose::new(fd);
		uhyve_send(UHYVE_PORT_CLOSE, &mut sysclose);

		sysclose.ret
	}

	/// ToDo: This function needs a description - also applies to trait in src/syscalls/interfaces/mod.rs
	///
	/// ToDo: Add Safety section under which circumctances this is safe/unsafe to use
	/// ToDo: Add an Errors section - What happens when e.g. malloc fails, how is that handled (currently it isn't)
	#[cfg(target_os = "none")]
	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		// determine the number of arguments and environment variables
		let mut syscmdsize = SysCmdsize::new();
		uhyve_send(UHYVE_PORT_CMDSIZE, &mut syscmdsize);

		// create array to receive all arguments
		let mut argv = Box::new(Vec::with_capacity(syscmdsize.argc as usize));
		let mut argv_phy = Vec::with_capacity(syscmdsize.argc as usize);
		for i in 0..syscmdsize.argc as usize {
			argv.push(crate::__sys_malloc(
				syscmdsize.argsz[i] as usize * mem::size_of::<u8>(),
				1,
			));
			argv_phy.push(
				paging::virtual_to_physical(VirtAddr(argv[i] as u64))
					.unwrap()
					.as_u64() as *const u8,
			);
		}

		// create array to receive the environment
		let mut env = Box::new(Vec::with_capacity(syscmdsize.envc as usize + 1));
		let mut env_phy = Vec::with_capacity(syscmdsize.envc as usize + 1);
		for i in 0..syscmdsize.envc as usize {
			env.push(crate::__sys_malloc(
				syscmdsize.envsz[i] as usize * mem::size_of::<u8>(),
				1,
			));
			env_phy.push(
				paging::virtual_to_physical(VirtAddr(env[i] as u64))
					.unwrap()
					.as_u64() as *const u8,
			);
		}

		// ask uhyve for the environment
		let mut syscmdval = SysCmdval::new(
			VirtAddr(argv_phy.as_ptr() as u64),
			VirtAddr(env_phy.as_ptr() as u64),
		);
		uhyve_send(UHYVE_PORT_CMDVAL, &mut syscmdval);

		let (argv_ptr, _, _) = argv.into_raw_parts();
		let (env_ptr, _, _) = env.into_raw_parts();
		(
			syscmdsize.argc,
			argv_ptr as *const *const u8,
			env_ptr as *const *const u8,
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
