use aarch64_cpu::asm::barrier::{SY, dsb, isb};
use aarch64_cpu::registers::{ReadWriteable, SCTLR_EL1, TTBR0_EL1, TTBR1_EL1, Writeable};
use log::info;

use super::RAM_START;
use super::paging::{BasePageSize, PageSize};

unsafe extern "C" {
	static mut l0_pgtable: u64;
	static mut l1_pgtable: u64;
	static mut l2_pgtable: u64;
	static mut l2k_pgtable: u64;
	static mut l3_pgtable: u64;
	static mut L0mib_pgtable: u64;
	static mut L2mib_pgtable: u64;
	static mut L4mib_pgtable: u64;
	static mut L6mib_pgtable: u64;
	static mut L8mib_pgtable: u64;
	static mut L10mib_pgtable: u64;
	static mut L12mib_pgtable: u64;
	static mut L14mib_pgtable: u64;
	static mut L16mib_pgtable: u64;
	static mut L18mib_pgtable: u64;
}

const PT_PT: u64 = 0x713;
const PT_MEM: u64 = 0x713;
const PT_MEM_CD: u64 = 0x70F;
const PT_SELF: u64 = 1 << 55;

pub unsafe fn init(uart_address: u32) {
	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(&raw mut l0_pgtable, 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = (&raw mut l1_pgtable).expose_provenance() as u64 + PT_PT;
	pgt_slice[511] = (&raw mut l0_pgtable).expose_provenance() as u64 + PT_PT + PT_SELF;

	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(&raw mut l1_pgtable, 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = (&raw mut l2_pgtable).expose_provenance() as u64 + PT_PT;
	pgt_slice[1] = (&raw mut l2k_pgtable).expose_provenance() as u64 + PT_PT;

	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(&raw mut l2_pgtable, 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = (&raw mut l3_pgtable).expose_provenance() as u64 + PT_PT;

	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(&raw mut l3_pgtable, 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[1] = uart_address as u64 + PT_MEM_CD;

	// map kernel to __executable_start and stack below the kernel
	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(&raw mut l2k_pgtable, 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}

	let mib_pgtables = [
		&raw mut L0mib_pgtable,
		&raw mut L2mib_pgtable,
		&raw mut L4mib_pgtable,
		&raw mut L6mib_pgtable,
		&raw mut L8mib_pgtable,
		&raw mut L10mib_pgtable,
		&raw mut L12mib_pgtable,
		&raw mut L14mib_pgtable,
		&raw mut L16mib_pgtable,
		&raw mut L18mib_pgtable,
	];

	for (mib_i, mib_pgtable) in mib_pgtables.into_iter().enumerate() {
		pgt_slice[mib_i] = mib_pgtable as u64 + PT_PT;

		let pgt_slice = unsafe { core::slice::from_raw_parts_mut(mib_pgtable, 512) };

		for (i, entry) in pgt_slice.iter_mut().enumerate() {
			let i = mib_i * 512 + i;
			*entry = RAM_START + (i * BasePageSize::SIZE) as u64 + PT_MEM;
		}
	}
}

pub unsafe fn enable() {
	// Set Translation Table Base Registers (TTBR)
	TTBR1_EL1.set(0);
	TTBR0_EL1.set((&raw mut l0_pgtable).expose_provenance() as u64);
	dsb(SY);
	isb(SY);

	// Set MMU enable in System Control Register (SCTLR)
	SCTLR_EL1.modify(SCTLR_EL1::M::Enable);
	isb(SY);

	info!("Successfully set up paging.");
}
