use core::ptr;

use memory_addresses::{PhysAddr, VirtAddr};
use uhyve_interface::v2::parameters::{ExitParams, SerialWriteBufferParams};
use uhyve_interface::v2::{Hypercall, HypercallAddress};

use crate::arch;
use crate::arch::mm::paging::{self, virtual_to_physical};
use crate::syscalls::interfaces::SyscallInterface;

/// perform a SerialWriteBuffer hypercall with `buf` as payload
#[inline]
#[cfg_attr(target_arch = "riscv64", expect(dead_code))]
pub(crate) fn serial_buf_hypercall(buf: &[u8]) {
	let p = SerialWriteBufferParams {
		buf: virtual_to_physical(VirtAddr::from_ptr(core::ptr::from_ref::<[u8]>(buf))).unwrap(),
		len: buf.len(),
	};
	uhyve_hypercall(Hypercall::SerialWriteBuffer(&p));
}

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
		Hypercall::Exit(exit_code) => u64::try_from(*exit_code).unwrap(),
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
	let mut ptr = HypercallAddress::from(&hypercall) as u64;
	let data = hypercall_data(&hypercall);

	#[cfg(target_arch = "x86_64")]
	{
		let ptr = ptr as *mut u64;
		unsafe { ptr.write_volatile(data) };
	}

	#[cfg(target_arch = "aarch64")]
	unsafe {
		use core::arch::asm;
		asm!(
			"str x8, [{ptr}]",
			ptr = in(reg) ptr,
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
		uhyve_hypercall(Hypercall::Exit(error_code));

		loop {
			arch::processor::halt();
		}
	}
}
