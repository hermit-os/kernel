use memory_addresses::VirtAddr;
use uhyve_interface::parameters::ExitParams;
use uhyve_interface::{Hypercall, HypercallAddress};
#[cfg(target_arch = "x86_64")]
use x86::io::*;

use crate::arch;
use crate::arch::mm::paging;
use crate::syscalls::interfaces::SyscallInterface;

#[inline]
/// calculates the physical address of the struct passed as reference.
fn data_addr<T>(data: &T) -> u64 {
	paging::virtual_to_physical(VirtAddr::from_ptr(data as *const T))
		.unwrap()
		.as_u64()
}

#[inline]
/// calculates the hypercall data argument
fn hypercall_data(hypercall: &Hypercall<'_>) -> u64 {
	match hypercall {
		Hypercall::Cmdsize(data) => data_addr(*data),
		Hypercall::Cmdval(data) => data_addr(*data),
		Hypercall::Exit(data) => data_addr(*data),
		Hypercall::FileClose(data) => data_addr(*data),
		Hypercall::FileLseek(data) => data_addr(*data),
		Hypercall::FileOpen(data) => data_addr(*data),
		Hypercall::FileRead(data) => data_addr(*data),
		Hypercall::FileUnlink(data) => data_addr(*data),
		Hypercall::FileWrite(data) => data_addr(*data),
		Hypercall::SerialWriteBuffer(data) => data_addr(*data),
		Hypercall::SerialWriteByte(byte) => *byte as u64,
		h => todo!("unimplemented hypercall {h:?}"),
	}
}

#[inline]
#[cfg(target_arch = "x86_64")]
/// Perform a hypercall to the uhyve hypervisor
pub(crate) fn uhyve_hypercall(hypercall: Hypercall<'_>) {
	let port = HypercallAddress::from(&hypercall) as u16;
	let data = hypercall_data(&hypercall);

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
	let ptr = HypercallAddress::from(&hypercall) as u16;
	let data = hypercall_data(&hypercall);
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
pub(crate) fn uhyve_hypercall(hypercall: Hypercall<'_>) {
	let _ptr = HypercallAddress::from(&hypercall) as u16;
	let _data = hypercall_data(&hypercall);
	todo!()
}

pub struct Uhyve;

impl SyscallInterface for Uhyve {
	fn shutdown(&self, error_code: i32) -> ! {
		let sysexit = ExitParams { arg: error_code };
		uhyve_hypercall(Hypercall::Exit(&sysexit));

		loop {
			arch::processor::halt();
		}
	}
}
