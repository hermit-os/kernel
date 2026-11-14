use core::arch::{asm, naked_asm};

use aarch64_cpu::asm::barrier::{SY, dsb};
use hermit_entry::Entry;
use hermit_entry::boot_info::RawBootInfo;

use crate::arch::kernel::scheduler::TaskStacks;
use crate::arch::kernel::{CPU_ONLINE, CURRENT_STACK_ADDRESS};
use crate::config::KERNEL_STACK_SIZE;
use crate::env;

/// Entrypoint - Initialize Stack pointer and Exception Table
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub unsafe extern "C" fn _start(boot_info: Option<&'static RawBootInfo>, cpu_id: u32) -> ! {
	// validate signatures
	// `_Start` is compatible to `Entry`
	{
		unsafe extern "C" fn _entry(_boot_info: &'static RawBootInfo, _cpu_id: u32) -> ! {
			unreachable!()
		}
		pub type _Start =
			unsafe extern "C" fn(boot_info: Option<&'static RawBootInfo>, cpu_id: u32) -> !;
		const _ENTRY: Entry = _entry;
		const _START: _Start = _start;
		const _PRE_INIT: _Start = pre_init;
	}

	naked_asm!(
		// use core::sync::atomic::{AtomicU32, Ordering};
		//
		// pub static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);
		//
		// while CPU_ONLINE.load(Ordering::Acquire) != this {
		//     core::hint::spin_loop();
		// }
		"mrs x4, mpidr_el1",
		"and x4, x4, #0xff",
		"1:",
		"adrp x8, {cpu_online}",
		"ldr x5, [x8, #:lo12:{cpu_online}]",
		"cmp x4, x5",
		"b.eq 2f",
		"b 1b",
		"2:",

		// we want to use sp_el1
		"msr spsel, #1",

		// Overwrite RSP if `CURRENT_STACK_ADDRESS != 0`
		"adrp x8, {current_stack_address}",
		"ldr x4, [x8, #:lo12:{current_stack_address}]",
		"cmp x4, 0",
		"b.eq 3f",
		"mov sp, x4",
		"b 4f",
		"3:",
		"mov x4, sp",
		"4:",
		"str x4, [x8, #:lo12:{current_stack_address}]",

		// Add stack top offset
		"mov x8, {stack_top_offset}",
		"add sp, sp, x8",

		// Jump to Rust code
		"b {pre_init}",

		cpu_online = sym CPU_ONLINE,
		stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
		current_stack_address = sym CURRENT_STACK_ADDRESS,
		pre_init = sym pre_init,
	)
}

#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pre_init(boot_info: Option<&'static RawBootInfo>, cpu_id: u32) -> ! {
	// set exception table
	unsafe {
		asm!(
			"adrp x4, vector_table",
			"add x4, x4, #:lo12:vector_table",
			"msr vbar_el1, x4",
			out("x4") _,
			options(nostack),
		);
	}

	// Memory barrier
	dsb(SY);

	// With FEAT_PAN, kernel accesses to user-accessible pages fault.
	// common-os needs them: set SCTLR_EL1.SPAN to keep PSTATE.PAN
	// unchanged on exception entry and clear PSTATE.PAN if implemented.
	#[cfg(feature = "common-os")]
	unsafe {
		asm!(
			"mrs {tmp}, sctlr_el1",
			"orr {tmp}, {tmp}, #(1 << 23)",
			"msr sctlr_el1, {tmp}",
			"isb",
			"mrs {tmp}, id_aa64mmfr1_el1",
			"ubfx {tmp}, {tmp}, #20, #4",
			"cbz {tmp}, 1f",
			".arch_extension pan",
			"msr pan, #0",
			"1:",
			tmp = out(reg) _,
			options(nostack, preserves_flags),
		);
	}

	if cpu_id == 0 {
		unsafe {
			env::set_start_info(*boot_info.unwrap());
		}
		crate::rt::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			let style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Red.into()));
			let preamble = format_args!("[            ][{cpu_id}][{style}ERROR{style:#}]");
			println!(
				"{preamble} Secondary core booted, but Hermit was not built with SMP support!"
			);
			loop {
				crate::arch::kernel::processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::rt::application_processor_main()
	}
}
