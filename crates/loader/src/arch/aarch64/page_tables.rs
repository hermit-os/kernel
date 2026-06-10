#![allow(non_upper_case_globals)]

use core::ptr;

use aarch64_cpu::asm::barrier::{SY, dsb, isb};
use aarch64_cpu::registers::{ReadWriteable, SCTLR_EL1, TTBR0_EL1, TTBR1_EL1, Writeable};
use log::info;

use super::RAM_START;
use super::paging::{BasePageSize, PageSize};

static mut l0_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut l1_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut l2_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut l2k_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut l3_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L0mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L2mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L4mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L6mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L8mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L10mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L12mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L14mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L16mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);
static mut L18mib_pgtable: PageTable = PageTable([ptr::null_mut(); _]);

const PT_PT: usize = 0x713;
const PT_MEM: usize = 0x713;
const PT_MEM_CD: usize = 0x70F;
const PT_SELF: usize = 1 << 55;

#[allow(static_mut_refs)] // FIXME: disallow
pub unsafe fn init(uart_address: u32) {
	let pgt = unsafe { &mut l0_pgtable.0 };
	for i in pgt.iter_mut() {
		*i = ptr::null_mut();
	}
	pgt[0] = (&raw mut l1_pgtable).wrapping_byte_add(PT_PT).cast();
	pgt[511] = (&raw mut l0_pgtable)
		.wrapping_byte_add(PT_PT)
		.wrapping_byte_add(PT_SELF)
		.cast();

	let pgt = unsafe { &mut l1_pgtable.0 };
	for i in pgt.iter_mut() {
		*i = ptr::null_mut();
	}
	pgt[0] = (&raw mut l2_pgtable).wrapping_byte_add(PT_PT).cast();
	pgt[1] = (&raw mut l2k_pgtable).wrapping_byte_add(PT_PT).cast();

	let pgt = unsafe { &mut l2_pgtable.0 };
	for i in pgt.iter_mut() {
		*i = ptr::null_mut();
	}
	pgt[0] = (&raw mut l3_pgtable).wrapping_byte_add(PT_PT).cast();

	let pgt = unsafe { &mut l3_pgtable.0 };
	for i in pgt.iter_mut() {
		*i = ptr::null_mut();
	}
	pgt[1] =
		ptr::with_exposed_provenance_mut::<()>(uart_address as usize).wrapping_byte_add(PT_MEM_CD);

	// map kernel to __executable_start and stack below the kernel
	let pgt = unsafe { &mut l2k_pgtable.0 };
	for i in pgt.iter_mut() {
		*i = ptr::null_mut();
	}

	let mib_pgtables = unsafe {
		[
			&mut L0mib_pgtable.0,
			&mut L2mib_pgtable.0,
			&mut L4mib_pgtable.0,
			&mut L6mib_pgtable.0,
			&mut L8mib_pgtable.0,
			&mut L10mib_pgtable.0,
			&mut L12mib_pgtable.0,
			&mut L14mib_pgtable.0,
			&mut L16mib_pgtable.0,
			&mut L18mib_pgtable.0,
		]
	};

	for (mib_pgt_i, mib_pgt) in mib_pgtables.into_iter().enumerate() {
		pgt[mib_pgt_i] = ptr::from_mut(mib_pgt).wrapping_byte_add(PT_PT).cast();

		for (entry_i, entry) in mib_pgt.iter_mut().enumerate() {
			let total_entry_i = mib_pgt_i * 512 + entry_i;
			*entry = ptr::with_exposed_provenance_mut::<()>(RAM_START as usize)
				.wrapping_byte_add(PT_MEM)
				.wrapping_byte_add(total_entry_i * BasePageSize::SIZE);
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

#[repr(C, align(0x1000))]
struct PageTable([*mut (); 512]);
