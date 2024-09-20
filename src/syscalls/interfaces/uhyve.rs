use alloc::alloc::{alloc, Layout};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::{mem, ptr};

use uhyve_interface::parameters::{CmdsizeParams, CmdvalParams, ExitParams};
use uhyve_interface::{GuestPhysAddr, Hypercall, MAX_ARGC_ENVC};
#[cfg(target_arch = "x86_64")]
use x86::io::*;

use crate::arch;
use crate::arch::mm::{paging, VirtAddr};
use crate::syscalls::interfaces::SyscallInterface;

#[inline]
/// calculates the physical address of the struct passed as reference.
fn data_addr<T>(data: &T) -> u64 {
	paging::virtual_to_physical(VirtAddr((data as *const T).addr() as u64))
		.unwrap()
		.as_u64()
}

#[inline]
/// calculates the physical address of the struct passed as reference.
fn data_addr_mut<T>(data: &mut T) -> u64 {
	paging::virtual_to_physical(VirtAddr((data as *mut T).addr() as u64))
		.unwrap()
		.as_u64()
}

#[inline]
/// calculates the hypercall data argument
fn hypercall_data(hypercall: Hypercall<'_>) -> u64 {
	match hypercall {
		Hypercall::Cmdsize(data) => data_addr_mut(data),
		Hypercall::Cmdval(data) => data_addr(data),
		Hypercall::Exit(data) => data_addr(data),
		Hypercall::FileClose(data) => data_addr_mut(data),
		Hypercall::FileLseek(data) => data_addr_mut(data),
		Hypercall::FileOpen(data) => data_addr_mut(data),
		Hypercall::FileRead(data) => data_addr_mut(data),
		Hypercall::FileUnlink(data) => data_addr_mut(data),
		Hypercall::FileWrite(data) => data_addr(data),
		Hypercall::SerialWriteBuffer(data) => data_addr(data),
		Hypercall::SerialWriteByte(byte) => byte as u64,
		h => todo!("unimplemented hypercall {h:?}"),
	}
}

#[inline]
#[cfg(target_arch = "x86_64")]
/// Perform a hypercall to the uhyve hypervisor
pub(crate) fn uhyve_hypercall(hypercall: Hypercall<'_>) {
	let port = hypercall.port();
	let data = hypercall_data(hypercall);

	unsafe {
		outl(
			port,
			data.try_into()
				.expect("Hypercall data must lie in the first 4GiB of memory"),
		);
	}
}

#[inline]
#[cfg(target_arch = "aarch64")]
/// Perform a hypercall to the uhyve hypervisor
pub(crate) fn uhyve_hypercall(hypercall: Hypercall<'_>) {
	let ptr = hypercall.port();
	let data = hypercall_data(hypercall);
	use core::arch::asm;

	unsafe {
		asm!(
			"str x8, [{ptr}]",
			ptr = in(reg) u64::from(ptr),
			in("x8") data,
			options(nostack),
		);
	}
}

#[inline]
#[cfg(target_arch = "riscv64")]
/// Perform a hypercall to the uhyve hypervisor
pub(crate) fn uhyve_hypercall<T>(_port: Hypercall) {
	todo!()
}

pub struct Uhyve;

impl SyscallInterface for Uhyve {
	/// ToDo: This function needs a description - also applies to trait in src/syscalls/interfaces/mod.rs
	///
	/// ToDo: Add Safety section under which circumctances this is safe/unsafe to use
	/// ToDo: Add an Errors section - What happens when e.g. malloc fails, how is that handled (currently it isn't)
	#[cfg(target_os = "none")]
	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		// determine the number of arguments and environment variables
		let mut syscmdsize = CmdsizeParams {
			argc: 0,
			argsz: [0; MAX_ARGC_ENVC],
			envc: 0,
			envsz: [0; MAX_ARGC_ENVC],
		};
		uhyve_hypercall(Hypercall::Cmdsize(&mut syscmdsize));

		// create array to receive all arguments
		let mut argv = Box::new(Vec::with_capacity(syscmdsize.argc as usize));
		let mut argv_phy = Vec::with_capacity(syscmdsize.argc as usize);
		for i in 0..syscmdsize.argc as usize {
			let layout =
				Layout::from_size_align(syscmdsize.argsz[i] as usize * mem::size_of::<u8>(), 1)
					.unwrap();

			argv.push(unsafe { alloc(layout).cast_const() });

			argv_phy.push(ptr::with_exposed_provenance::<u8>(
				paging::virtual_to_physical(VirtAddr(argv[i] as u64))
					.unwrap()
					.as_usize(),
			));
		}

		// create array to receive the environment
		let mut env = Box::new(Vec::with_capacity(syscmdsize.envc as usize + 1));
		let mut env_phy = Vec::with_capacity(syscmdsize.envc as usize + 1);
		for i in 0..syscmdsize.envc as usize {
			let layout =
				Layout::from_size_align(syscmdsize.envsz[i] as usize * mem::size_of::<u8>(), 1)
					.unwrap();
			env.push(unsafe { alloc(layout).cast_const() });

			env_phy.push(ptr::with_exposed_provenance::<u8>(
				paging::virtual_to_physical(VirtAddr(env[i] as u64))
					.unwrap()
					.as_usize(),
			));
		}
		env.push(ptr::null::<u8>());

		// ask uhyve for the environment
		let cmdval_params = CmdvalParams {
			argv: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr(argv_phy.as_ptr() as u64))
					.unwrap()
					.0,
			),
			envp: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr(env_phy.as_ptr() as u64))
					.unwrap()
					.0,
			),
		};
		uhyve_hypercall(Hypercall::Cmdval(&cmdval_params));

		let argv = argv.leak().as_ptr();
		let env = env.leak().as_ptr();
		(syscmdsize.argc, argv, env)
	}

	fn shutdown(&self, error_code: i32) -> ! {
		let sysexit = ExitParams { arg: error_code };
		uhyve_hypercall(Hypercall::Exit(&sysexit));

		loop {
			arch::processor::halt();
		}
	}
}
