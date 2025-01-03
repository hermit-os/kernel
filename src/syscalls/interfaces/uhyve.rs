use core::ptr;

use memory_addresses::VirtAddr;
use uhyve_interface::parameters::ExitParams;
use uhyve_interface::{Hypercall, HypercallAddress};

use crate::arch;
use crate::arch::mm::paging;
use crate::syscalls::interfaces::SyscallInterface;

/// calculates the physical address of the struct passed as reference.
#[inline]
fn data_addr<T>(data: &T) -> u64 {
	paging::virtual_to_physical(VirtAddr::from_ptr(ptr::from_ref(data)))
		.unwrap()
		.as_u64()
}

/// calculates the hypercall data argument
#[inline]
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
		Hypercall::SerialWriteByte(byte) => u64::from(*byte),
		h => todo!("unimplemented hypercall {h:?}"),
	}
}

/// Perform a hypercall to the uhyve hypervisor
#[inline]
#[allow(unused_variables)] // until riscv64 is implemented
pub(crate) fn uhyve_hypercall(hypercall: Hypercall<'_>) {
	let ptr = HypercallAddress::from(&hypercall) as u16;
	let data = hypercall_data(&hypercall);

	#[cfg(target_arch = "x86_64")]
	unsafe {
		use x86_64::instructions::port::Port;

		let data =
			u32::try_from(data).expect("Hypercall data must lie in the first 4GiB of memory");
		Port::new(ptr).write(data);
	}

	#[cfg(target_arch = "aarch64")]
	unsafe {
		use core::arch::asm;
		asm!(
			"str x8, [{ptr}]",
			ptr = in(reg) u64::from(ptr),
			in("x8") data,
			options(nostack),
		);
	}

	#[cfg(target_arch = "riscv64")]
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
