#![allow(dead_code)]

use core::arch::{asm, naked_asm};
#[cfg(feature = "smp")]
use core::sync::atomic::AtomicPtr;

use hermit_entry::Entry;
use hermit_entry::boot_info::RawBootInfo;

use crate::arch::aarch64::kernel::scheduler::TaskStacks;
use crate::{KERNEL_STACK_SIZE, env};

/*
 * Memory types available.
 */
#[allow(non_upper_case_globals)]
const MT_DEVICE_nGnRnE: u64 = 0;
#[allow(non_upper_case_globals)]
const MT_DEVICE_nGnRE: u64 = 1;
const MT_DEVICE_GRE: u64 = 2;
const MT_NORMAL_NC: u64 = 3;
const MT_NORMAL: u64 = 4;

/*
 * TCR flags
 */
const TCR_IRGN_WBWA: u64 = ((1) << 8) | ((1) << 24);
const TCR_ORGN_WBWA: u64 = ((1) << 10) | ((1) << 26);
const TCR_SHARED: u64 = ((3) << 12) | ((3) << 28);
const TCR_TBI0: u64 = 1 << 37;
const TCR_TBI1: u64 = 1 << 38;
const TCR_ASID16: u64 = 1 << 36;
const TCR_TG1_16K: u64 = 1 << 30;
const TCR_TG1_4K: u64 = 0 << 30;
const TCR_FLAGS: u64 = TCR_IRGN_WBWA | TCR_ORGN_WBWA | TCR_SHARED;

/// Number of virtual address bits for 4KB page
const VA_BITS: u64 = 48;

unsafe extern "C" {
	static vector_table: u8;
}

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

		cpu_online = sym super::CPU_ONLINE,
		stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
		current_stack_address = sym super::CURRENT_STACK_ADDRESS,
		pre_init = sym pre_init,
	)
}

#[cfg(feature = "smp")]
const fn tcr_size(x: u64) -> u64 {
	((64 - x) << 16) | (64 - x)
}

#[cfg(feature = "smp")]
const fn mair(attr: u64, mt: u64) -> u64 {
	attr << (mt * 8)
}

#[cfg(feature = "smp")]
pub(crate) static TTBR0: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

#[cfg(feature = "smp")]
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn smp_start() -> ! {
	naked_asm!(
		// disable interrupts
		"msr daifset, #0b111",

		// we want to use sp_el1!
		"msr spsel, #1",

		// reset thread id registers
		"msr tpidr_el0, xzr",
		"msr tpidr_el1, xzr",

		// Disable the MMU
		"dsb sy",
		"mrs x2, sctlr_el1",
		"bic x2, x2, #0x1",
		"msr sctlr_el1, x2",
		"isb",

		"ic iallu",
		"tlbi vmalle1is",
		"dsb ish",

		// Setup memory attribute type tables
		"ldr x1, ={mair_el1}",
		"msr mair_el1, x1",

		// Setup translation control register (TCR)
		"mrs x0, id_aa64mmfr0_el1",
		"and x0, x0, 0xF",
		"lsl x0, x0, 32",
		"ldr x1, ={tcr_bits}",
		"orr x0, x0, x1",
		"mrs x1, id_aa64mmfr0_el1",
		"bfi x0, x1, #32, #3",
		"msr tcr_el1, x0",

		// Enable FP/ASIMD in Architectural Feature Access Control Register,
		"mov x0, 3",
		"lsl x0, x0, 20",
		"msr cpacr_el1, x0",

		// Reset debug control register
		"msr mdscr_el1, xzr",

		// Memory barrier
		"dsb sy",

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

		"msr ttbr1_el1, xzr",
		"adrp x8, {ttbr0}",
		"ldr x5, [x8, #:lo12:{ttbr0}]",
		"msr ttbr0_el1, x5",

		// Prepare system control register (SCTRL)
		//
		// UCI     [26] Enables EL0 access in AArch64 for DC CVAU, DC CIVAC,
		//				DC CVAC and IC IVAU instructions
		// EE      [25] Explicit data accesses at EL1 and Stage 1 translation
		//				table walks at EL1 & EL0 are little-endian
		// EOE     [24] Explicit data accesses at EL0 are little-endian
		// WXN     [19] Regions with write permission are not forced to XN
		// nTWE    [18] WFE instructions are executed as normal
		// nTWI    [16] WFI instructions are executed as normal
		// UCT     [15] Enables EL0 access in AArch64 to the CTR_EL0 register
		// DZE     [14] Execution of the DC ZVA instruction is allowed at EL0
		// I       [12] Instruction caches enabled at EL0 and EL1
		// UMA     [9]  Disable access to the interrupt masks from EL0
		// SED     [8]  The SETEND instruction is available
		// ITD     [7]  The IT instruction functionality is available
		// THEE    [6]  ThumbEE is disabled
		// CP15BEN [5]  CP15 barrier operations disabled
		// SA0     [4]  Stack Alignment check for EL0 enabled
		// SA      [3]  Stack Alignment check enabled
		// C       [2]  Data and unified enabled
		// A       [1]  Alignment fault checking disabled
		// M       [0]  MMU enable
		"ldr x0, =0x405d01d",
		  "msr sctlr_el1, x0",

		// initialize argument for pre_init
		"mov x0, xzr",
		"mrs x1, mpidr_el1",
		"and x1, x1, #0xff",

		// Jump to Rust code
		"b {pre_init}",

		mair_el1 = const mair(0x00, MT_DEVICE_nGnRnE) | mair(0x04, MT_DEVICE_nGnRE) | mair(0x0c, MT_DEVICE_GRE) | mair(0x44, MT_NORMAL_NC) | mair(0xff, MT_NORMAL),
		tcr_bits = const tcr_size(VA_BITS) | TCR_TG1_4K | TCR_FLAGS,
		stack_top_offset = const KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE,
		current_stack_address = sym super::CURRENT_STACK_ADDRESS,
		ttbr0 = sym TTBR0,
		pre_init = sym pre_init,
	)
}

#[inline(never)]
#[unsafe(no_mangle)]
unsafe extern "C" fn pre_init(boot_info: Option<&'static RawBootInfo>, cpu_id: u32) -> ! {
	// set exception table
	unsafe {
		asm!(
			"adrp x4, {vector_table}",
			"add  x4, x4, #:lo12:{vector_table}",
			"msr vbar_el1, x4",
			vector_table = sym vector_table,
			out("x4") _,
			options(nostack),
		);

		// Memory barrier
		asm!("dsb sy", options(nostack),);
	}

	if cpu_id == 0 {
		env::set_boot_info(*boot_info.unwrap());
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				crate::arch::processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main()
	}
}
