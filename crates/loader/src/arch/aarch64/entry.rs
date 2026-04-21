#![allow(dead_code)]

use core::arch::{asm, global_asm};

use aarch64_cpu::asm::{barrier, wfe};
use aarch64_cpu::registers::{
	CPACR_EL1, ID_AA64MMFR0_EL1, MAIR_EL1, MDSCR_EL1, ReadWriteable, Readable, SCTLR_EL1, TCR_EL1,
	TPIDR_EL0, TPIDR_EL1, Writeable,
};
use log::info;
use tock_registers::fields::{FieldValue, TryFromValue};

const BOOT_CORE_ID: u64 = 0; // ID of CPU for booting on SMP systems - this might be board specific in the future

/// Number of virtual address bits for 4KB page
const VA_BITS: u64 = 48;

global_asm!(
	include_str!("entry.s"),
	start_rust = sym start_rust,
);

#[inline(never)]
pub unsafe fn start_rust() -> ! {
	unsafe { pre_init() }
}

unsafe fn pre_init() -> ! {
	crate::log::init();
	info!("Enter startup code");

	/* disable interrupts */
	/*
	 * FIXME: Migrate to aarch64_cpu's DAIFSet definition once released,
	 * see https://github.com/rust-embedded/aarch64-cpu/pull/76
	 */
	unsafe {
		asm!("msr daifset, 0b111", options(nostack));
	}

	/* reset thread id registers */
	TPIDR_EL0.set(0);
	TPIDR_EL1.set(0);

	/*
	 * Disable the MMU. We may have entered the kernel with it on and
	 * will need to update the tables later. If this has been set up
	 * with anything other than a VA == PA map then this will fail,
	 * but in this case the code to find where we are running from
	 * would have also failed.
	 */
	barrier::dsb(barrier::SY);
	SCTLR_EL1.modify(SCTLR_EL1::M::Disable);
	barrier::isb(barrier::SY);

	unsafe {
		asm!("ic iallu", "tlbi vmalle1is", options(nostack));
	}
	barrier::dsb(barrier::ISH);

	/*
	 * Setup memory attribute type tables
	 */
	MAIR_EL1.write(
		MAIR_EL1::Attr0_Device::nonGathering_nonReordering_noEarlyWriteAck
			+ MAIR_EL1::Attr1_Device::nonGathering_nonReordering_EarlyWriteAck
			+ MAIR_EL1::Attr2_Device::Gathering_Reordering_EarlyWriteAck
			+ MAIR_EL1::Attr3_Normal_Inner::NonCacheable
			+ MAIR_EL1::Attr3_Normal_Outer::NonCacheable
			+ MAIR_EL1::Attr4_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc
			+ MAIR_EL1::Attr4_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc,
	);

	/*
	 * Setup translation control register (TCR)
	 */

	// determine physical address size
	let Some(pa_range) = ID_AA64MMFR0_EL1
		.read_as_enum::<ID_AA64MMFR0_EL1::PARange::Value>(ID_AA64MMFR0_EL1::PARange)
	else {
		panic!("Unknown physical address range")
	};
	let Some(ips) = TCR_EL1::IPS::Value::try_from_value(pa_range as u64) else {
		panic!("Invalid physical address size")
	};

	TCR_EL1.write(
		FieldValue::from(ips)
			+ TCR_EL1::T1SZ.val(64 - VA_BITS)
			+ TCR_EL1::T0SZ.val(64 - VA_BITS)
			+ TCR_EL1::TG1::KiB_4
			+ TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
			+ TCR_EL1::IRGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
			+ TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
			+ TCR_EL1::ORGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
			+ TCR_EL1::SH0::Inner
			+ TCR_EL1::SH1::Inner,
	);

	/*
	 * Enable FP/ASIMD in Architectural Feature Access Control Register,
	 */
	CPACR_EL1.write(CPACR_EL1::FPEN::TrapNothing);

	/*
	 * Reset debug control register
	 */
	MDSCR_EL1.set(0);

	/* Memory barrier */
	barrier::dsb(barrier::SY);

	/*
	* Prepare system control register (SCTRL)
	* Todo: - Verify if all of these bits actually should be explicitly set
		   - Link origin of this documentation and check to which instruction set versions
			 it applies (if applicable)
		   - Fill in the missing Documentation for some of the bits and verify if we care about them
			 or if loading and not setting them would be the appropriate action.
	*/

	#[cfg(target_endian = "big")]
	let endian = SCTLR_EL1::EE::BigEndian + SCTLR_EL1::E0E::BigEndian;
	#[cfg(target_endian = "little")]
	let endian = SCTLR_EL1::EE::LittleEndian + SCTLR_EL1::E0E::LittleEndian;

	SCTLR_EL1.write(
		SCTLR_EL1::UCI::DontTrap
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
			+ SCTLR_EL1::M::Disable
			+ endian,
	);

	// Enter loader
	unsafe {
		crate::os::loader_main();
	}
}

pub fn wait_forever() -> ! {
	loop {
		wfe();
	}
}
