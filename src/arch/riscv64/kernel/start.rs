use core::arch::naked_asm;
use core::sync::atomic::Ordering;

use fdt::Fdt;
use hermit_entry::Entry;
use hermit_entry::boot_info::RawBootInfo;

use super::{CPU_ONLINE, CURRENT_BOOT_ID, HART_MASK, NUM_CPUS, get_dtb_ptr};
use crate::arch::riscv64::kernel::CURRENT_STACK_ADDRESS;
#[cfg(not(feature = "smp"))]
use crate::arch::riscv64::kernel::processor;
use crate::{KERNEL_STACK_SIZE, env};

//static mut BOOT_STACK: [u8; KERNEL_STACK_SIZE] = [0; KERNEL_STACK_SIZE];

/// Entrypoint - Initialize Stack pointer and Exception Table
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start(hart_id: usize, boot_info: Option<&'static RawBootInfo>) -> ! {
	// validate signatures
	// `_Start` is compatible to `Entry`
	{
		unsafe extern "C" fn _entry(_hart_id: usize, _boot_info: &'static RawBootInfo) -> ! {
			unreachable!()
		}
		pub type _Start =
			unsafe extern "C" fn(hart_id: usize, boot_info: Option<&'static RawBootInfo>) -> !;
		const _ENTRY: Entry = _entry;
		const _START: _Start = _start;
		const _PRE_INIT: _Start = pre_init;
	}

	unsafe {
		naked_asm!(
			// Use stack pointer from `CURRENT_STACK_ADDRESS` if set
			"ld      t0, {current_stack_pointer}",
			"beqz    t0, 2f",
			"li      t1, {top_offset}",
			"add     t0, t0, t1",
			"mv      sp, t0",
			"2:",

			"j       {pre_init}",
			current_stack_pointer = sym CURRENT_STACK_ADDRESS,
			top_offset = const KERNEL_STACK_SIZE,
			pre_init = sym pre_init,
		)
	}
}

unsafe extern "C" fn pre_init(hart_id: usize, boot_info: Option<&'static RawBootInfo>) -> ! {
	CURRENT_BOOT_ID.store(hart_id as u32, Ordering::Relaxed);

	if CPU_ONLINE.load(Ordering::Acquire) == 0 {
		unsafe {
			env::set_boot_info(*boot_info.unwrap());
			let fdt = Fdt::from_ptr(get_dtb_ptr()).expect("FDT is invalid");
			// Init HART_MASK
			let mut hart_mask = 0;
			for cpu in fdt.cpus() {
				let hart_id = cpu.property("reg").unwrap().as_usize().unwrap();
				let status = cpu.property("status").unwrap().as_str().unwrap();

				if status != "disabled\u{0}" {
					hart_mask |= 1 << hart_id;
				}
			}
			NUM_CPUS.store(fdt.cpus().count().try_into().unwrap(), Ordering::Relaxed);
			HART_MASK.store(hart_mask, Ordering::Relaxed);
		}
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}
