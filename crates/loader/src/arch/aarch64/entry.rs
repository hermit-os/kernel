#![allow(dead_code)]

use core::arch::{asm, global_asm};

use aarch64_cpu::registers::{Writeable, SCTLR_EL1};
use log::info;

extern "C" {
	fn loader_main();
}

const BOOT_CORE_ID: u64 = 0; // ID of CPU for booting on SMP systems - this might be board specific in the future

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

#[inline(always)]
const fn mair(attr: u64, mt: u64) -> u64 {
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

#[inline(always)]
const fn tcr_size(x: u64) -> u64 {
	((64 - x) << 16) | (64 - x)
}

global_asm!(include_str!("entry.s"));

#[inline(never)]
#[no_mangle]
#[link_section = ".text._start"]
pub unsafe fn _start_rust() -> ! {
	unsafe { pre_init() }
}

unsafe fn pre_init() -> ! {
	info!("Enter startup code");

	/* disable interrupts */
	unsafe {
		asm!("msr daifset, 0b111", options(nostack));
	}

	/* reset thread id registers */
	unsafe {
		asm!("msr tpidr_el0, xzr", "msr tpidr_el1, xzr", options(nostack));
	}

	/*
	 * Disable the MMU. We may have entered the kernel with it on and
	 * will need to update the tables later. If this has been set up
	 * with anything other than a VA == PA map then this will fail,
	 * but in this case the code to find where we are running from
	 * would have also failed.
	 */
	unsafe {
		asm!("dsb sy",
			"mrs x2, sctlr_el1",
			"bic x2, x2, 0x1",
			"msr sctlr_el1, x2",
			"isb",
			out("x2") _,
			options(nostack),
		);
	}

	unsafe {
		asm!("ic iallu", "tlbi vmalle1is", "dsb ish", options(nostack));
	}

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
	unsafe {
		asm!("msr mair_el1, {}",
			in(reg) mair_el1,
			options(nostack),
		);
	}

	/*
	 * Setup translation control register (TCR)
	 */

	// determine physical address size
	unsafe {
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
	}

	/*
	 * Enable FP/ASIMD in Architectural Feature Access Control Register,
	 */
	let bit_mask: u64 = 3 << 20;
	unsafe {
		asm!("msr cpacr_el1, {mask}",
			mask = in(reg) bit_mask,
			options(nostack),
		);
	}

	/*
	 * Reset debug control register
	 */
	unsafe {
		asm!("msr mdscr_el1, xzr", options(nostack));
	}

	/* Memory barrier */
	unsafe {
		asm!("dsb sy", options(nostack));
	}

	/*
	* Prepare system control register (SCTRL)
	* Todo: - Verify if all of these bits actually should be explicitly set
		   - Link origin of this documentation and check to which instruction set versions
			 it applies (if applicable)
		   - Fill in the missing Documentation for some of the bits and verify if we care about them
			 or if loading and not setting them would be the appropriate action.
	*/

	SCTLR_EL1.write(
		SCTLR_EL1::UCI::DontTrap
			+ SCTLR_EL1::EE::LittleEndian
			+ SCTLR_EL1::E0E::LittleEndian
			+ SCTLR_EL1::WXN::Disable
			+ SCTLR_EL1::NTWE::DontTrap
			+ SCTLR_EL1::NTWI::DontTrap
			+ SCTLR_EL1::UCT::DontTrap
			+ SCTLR_EL1::DZE::DontTrap
			+ SCTLR_EL1::I::Cacheable
			+ SCTLR_EL1::UMA::Trap
			+ SCTLR_EL1::NAA::Disable
			+ SCTLR_EL1::SA0::Enable
			+ SCTLR_EL1::SA::Enable
			+ SCTLR_EL1::C::Cacheable
			+ SCTLR_EL1::A::Disable
			+ SCTLR_EL1::M::Disable,
	);

	// Enter loader
	unsafe {
		loader_main();
	}

	unreachable!()
}

pub unsafe fn wait_forever() -> ! {
	loop {
		unsafe {
			asm!("wfe");
		}
	}
}
