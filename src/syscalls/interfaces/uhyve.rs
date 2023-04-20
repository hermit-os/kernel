use alloc::alloc::{alloc, Layout};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem;

#[cfg(target_arch = "x86_64")]
use x86::io::*;

use crate::arch;
use crate::arch::mm::{paging, PhysAddr, VirtAddr};
use crate::syscalls::interfaces::SyscallInterface;
#[cfg(feature = "newlib")]
use crate::syscalls::lwip::sys_lwip_get_errno;
#[cfg(feature = "newlib")]
use crate::syscalls::{LWIP_FD_BIT, LWIP_LOCK};

pub(crate) const UHYVE_PORT_EXIT: u16 = 0x540;
pub(crate) const UHYVE_PORT_CMDSIZE: u16 = 0x740;
pub(crate) const UHYVE_PORT_CMDVAL: u16 = 0x780;
pub(crate) const UHYVE_PORT_UNLINK: u16 = 0x840;

#[cfg(feature = "newlib")]
extern "C" {
	fn lwip_write(fd: i32, buf: *const u8, len: usize) -> i32;
	fn lwip_read(fd: i32, buf: *mut u8, len: usize) -> i32;
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "x86_64")]
pub(crate) fn uhyve_send<T>(port: u16, data: &mut T) {
	let ptr = VirtAddr(data as *mut _ as u64);
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	unsafe {
		outl(port, physical_address.as_u64() as u32);
	}
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "aarch64")]
pub(crate) fn uhyve_send<T>(port: u16, data: &mut T) {
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

pub struct Uhyve;

impl SyscallInterface for Uhyve {
	fn unlink(&self, name: *const u8) -> i32 {
		let mut sysunlink = SysUnlink::new(VirtAddr(name as u64));
		uhyve_send(UHYVE_PORT_UNLINK, &mut sysunlink);

		sysunlink.ret
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
			let layout =
				Layout::from_size_align(syscmdsize.argsz[i] as usize * mem::size_of::<u8>(), 1)
					.unwrap();

			argv.push(unsafe { alloc(layout).cast_const() });

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
			let layout =
				Layout::from_size_align(syscmdsize.envsz[i] as usize * mem::size_of::<u8>(), 1)
					.unwrap();
			env.push(unsafe { alloc(layout).cast_const() });

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

		let argv = argv.leak().as_ptr();
		let env = env.leak().as_ptr();
		(syscmdsize.argc, argv, env)
	}

	fn shutdown(&self, arg: i32) -> ! {
		let mut sysexit = SysExit::new(arg);
		uhyve_send(UHYVE_PORT_EXIT, &mut sysexit);

		loop {
			arch::processor::halt();
		}
	}
}
