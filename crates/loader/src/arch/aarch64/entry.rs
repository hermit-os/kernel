// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![allow(dead_code)]

extern "C" {
	fn loader_main();
}

const BOOT_STACK_SIZE: usize = 4096;
const BOOT_CORE_ID: u64 = 0; // ID of CPU for booting on SMP systems - this might be board specific in the future

#[link_section = ".data"]
static STACK: [u8; BOOT_STACK_SIZE] = [0; BOOT_STACK_SIZE];

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

fn mair(attr: u64, mt: u64) -> u64 {
	attr << (mt * 8)
}

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

fn tcr_size(x: u64) -> u64 {
	(((64) - (x)) << 16) | (((64) - (x)) << 0)
}

global_asm!(include_str!("entry.s"));

#[inline(never)]
#[no_mangle]
pub unsafe fn _start_rust() -> ! {
	// Pointer to stack base
	llvm_asm!("mov sp, $0"
		:: "r"(&STACK[BOOT_STACK_SIZE - 0x10] as *const u8 as usize)
        :: "volatile");
	pre_init()
}

unsafe fn pre_init() -> ! {
	loaderlog!("Enter startup code");

	/* disable interrupts */
	asm!(
		"msr daifset, {mask}",
		mask = const 0b111,
		options(nostack),
	);

	/* reset thread id registers */
	asm!("msr tpidr_el0, {0}",
		"msr tpidr_el1, {0}",
		in(reg) 0_u64,
		options(nostack),
	);

	/*
	 * Disable the MMU. We may have entered the kernel with it on and
	 * will need to update the tables later. If this has been set up
	 * with anything other than a VA == PA map then this will fail,
	 * but in this case the code to find where we are running from
	 * would have also failed.
	 */
	asm!("dsb sy",
		"mrs x2, sctlr_el1",
		"bic x2, x2, {one}",
		"msr sctlr_el1, x2",
		"isb",
		one = const 0x1,
		out("x2") _,
		options(nostack),
		//::: "x2" : "volatile"
	);

	asm!("ic iallu", "tlbi vmalle1is", "dsb ish", options(nostack),);

	/*
	 * Setup memory attribute type tables
	 *
	 * Memory regioin attributes for LPAE:
	 *
	 *   n = AttrIndx[2:0]
	 *                      n       MAIR
	 *   DEVICE_nGnRnE      000     00000000 (0x00)
	 *   DEVICE_nGnRE       001     00000100 (0x04)
	 *   DEVICE_GRE         010     00001100 (0x0c)
	 *   NORMAL_NC          011     01000100 (0x44)
	 *   NORMAL             100     11111111 (0xff)
	 */
	let mair_el1 = mair(0x00, MT_DEVICE_nGnRnE)
		| mair(0x04, MT_DEVICE_nGnRE)
		| mair(0x0c, MT_DEVICE_GRE)
		| mair(0x44, MT_NORMAL_NC)
		| mair(0xff, MT_NORMAL);
	asm!("msr mair_el1, {0}",
		in(reg) mair_el1,
		options(nostack),
	);

	/*
	 * Setup translation control register (TCR)
	 */

	// determine physical address size
	asm!("mrs x0, id_aa64mmfr0_el1",
		"and x0, x0, 0xF",
		"lsl x0, x0, 32",
		"orr x0, x0, {tcr_bits}",
		"mrs x1, id_aa64mmfr0_el1",
		"bfi x0, x1, #32, #3",
		"msr tcr_el1, x0",
		tcr_bits = in(reg) tcr_size(VA_BITS) | TCR_TG1_4K | TCR_FLAGS,
		out("x0") _,
		out("x1") _,
	);

	/*
	 * Enable FP/ASIMD in Architectural Feature Access Control Register,
	 */
	let bit_mask: u64 = 3 << 20;
	asm!("msr cpacr_el1, {0}",
		in(reg) bit_mask,
		options(nostack),
	);

	/*
	 * Reset debug control register
	 */
	asm!("msr mdscr_el1, xzr", options(nostack));

	/* Turning on MMU */
	asm!("dsb sy", options(nostack));

	/*
	* Prepare system control register (SCTRL)
	* Todo: - Verify if all of these bits actually should be explicitly set
			- Link origin of this documentation and check to which instruction set versions
			  it applies (if applicable)
			- Fill in the missing Documentation for some of the bits and verify if we care about them
			  or if loading ond not setting them would be the appropriate action.
	*/
	let sctrl_el1: u64 = 0
		| (1 << 26) 	/* UCI     	Enables EL0 access in AArch64 for DC CVAU, DC CIVAC,
					 				DC CVAC and IC IVAU instructions */
		| (0 << 25)		/* EE      	Explicit data accesses at EL1 and Stage 1 translation
					 				table walks at EL1 & EL0 are little-endian*/
		| (0 << 24)		/* EOE     	Explicit data accesses at EL0 are little-endian*/
		| (1 << 23)
		| (1 << 22)
		| (1 << 20)
		| (0 << 19)		/* WXN     	Regions with write permission are not forced to XN */
		| (1 << 18)		/* nTWE     WFE instructions are executed as normal*/
		| (0 << 17)
		| (1 << 16)		/* nTWI    	WFI instructions are executed as normal*/
		| (1 << 15)		/* UCT     	Enables EL0 access in AArch64 to the CTR_EL0 register*/
		| (1 << 14)		/* DZE     	Execution of the DC ZVA instruction is allowed at EL0*/
		| (0 << 13)
		| (1 << 12)		/* I       	Instruction caches enabled at EL0 and EL1*/
		| (1 << 11)
		| (0 << 10)
		| (0 << 9)		/* UMA      Disable access to the interrupt masks from EL0*/
		| (1 << 8)		/* SED      The SETEND instruction is available*/
		| (0 << 7)		/* ITD      The IT instruction functionality is available*/
		| (0 << 6)		/* THEE    	ThumbEE is disabled*/
		| (0 << 5)		/* CP15BEN  CP15 barrier operations disabled*/
		| (1 << 4)		/* SA0     	Stack Alignment check for EL0 enabled*/
		| (1 << 3)		/* SA      	Stack Alignment check enabled*/
		| (1 << 2)		/* C       	Data and unified enabled*/
		| (0 << 1)		/* A       	Alignment fault checking disabled*/
		| (0 << 0)		/* M       	MMU enable*/
		;
	asm!("msr sctlr_el1, {0}", in(reg) sctrl_el1, options(nostack));

	// Enter loader
	loader_main();

	// we should never reach this  point
	loop {}
}

pub unsafe fn wait_forever() -> ! {
	loop {
		asm!("wfe")
	}
}
