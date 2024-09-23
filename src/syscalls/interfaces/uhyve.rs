use core::ptr;

use crate::arch;
use crate::arch::mm::{paging, VirtAddr};
use crate::syscalls::interfaces::SyscallInterface;

const UHYVE_PORT_EXIT: u16 = 0x540;

/// forward a request to the hypervisor uhyve
#[inline]
fn uhyve_send<T>(port: u16, data: &mut T) {
	let ptr = VirtAddr(ptr::from_mut(data).addr() as u64);
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	#[cfg(target_arch = "x86_64")]
	unsafe {
		x86::io::outl(port, physical_address.as_u64() as u32);
	}

	#[cfg(target_arch = "aarch64")]
	unsafe {
		core::arch::asm!(
			"str x8, [{port}]",
			port = in(reg) u64::from(port),
			in("x8") physical_address.as_u64(),
			options(nostack),
		);
	}

	#[cfg(target_arch = "riscv64")]
	todo!("uhyve_send(port = {port}, physical_address = {physical_address})");
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

pub struct Uhyve;

impl SyscallInterface for Uhyve {
	fn shutdown(&self, error_code: i32) -> ! {
		let mut sysexit = SysExit::new(error_code);
		uhyve_send(UHYVE_PORT_EXIT, &mut sysexit);

		loop {
			arch::processor::halt();
		}
	}
}
