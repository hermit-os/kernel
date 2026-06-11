use core::ptr;

use aarch64_cpu::asm::barrier::{SY, dsb, isb};
use aarch64_cpu::registers::{ReadWriteable, SCTLR_EL1, TTBR0_EL1, TTBR1_EL1, Writeable};
use log::info;

use super::RAM_START;
use super::paging::{BasePageSize, PageSize};

static mut LEVEL_0_TABLE: PageTable = {
	let mut table = [ptr::null_mut(); _];

	table[0] = (&raw mut LEVEL_1_TABLE)
		.wrapping_byte_add(descr::NORMAL)
		.cast();
	table[511] = (&raw mut LEVEL_0_TABLE)
		.wrapping_byte_add(descr::NORMAL)
		.wrapping_byte_add(descr::SELF)
		.cast();

	PageTable(table)
};

static mut LEVEL_1_TABLE: PageTable = {
	let mut table = [ptr::null_mut(); _];

	table[0] = (&raw mut LEVEL_2_TABLE_SERIAL)
		.wrapping_byte_add(descr::NORMAL)
		.cast();
	table[1] = (&raw mut LEVEL_2_TABLE_RAM)
		.wrapping_byte_add(descr::NORMAL)
		.cast();

	PageTable(table)
};

static mut LEVEL_2_TABLE_SERIAL: PageTable = {
	let mut table = [ptr::null_mut(); _];

	table[0] = (&raw mut LEVEL_3_TABLE_SERIAL)
		.wrapping_byte_add(descr::NORMAL)
		.cast();

	PageTable(table)
};

static mut LEVEL_2_TABLE_RAM: PageTable = {
	let mut table = [ptr::null_mut(); _];

	let mut entry_i = 0;
	while entry_i < 10usize {
		let entry = &mut table[entry_i];

		let level_3_table = unsafe { &raw mut LEVEL_3_TABLES_RAM[entry_i] };
		*entry = level_3_table.wrapping_byte_add(descr::NORMAL).cast();

		entry_i += 1;
	}

	PageTable(table)
};

static mut LEVEL_3_TABLE_SERIAL: PageTable = PageTable([ptr::null_mut(); _]);

static mut LEVEL_3_TABLES_RAM: [PageTable; 10] = {
	let mut tables = [PageTable([ptr::null_mut(); _]); _];

	let mut table_i = 0;
	while table_i < tables.len() {
		let table = &mut tables[table_i].0;

		let mut entry_i = 0;
		while entry_i < table.len() {
			let entry = &mut table[entry_i];

			let addr = (table_i * 512 + entry_i) * BasePageSize::SIZE;
			*entry = ptr::with_exposed_provenance_mut::<()>(RAM_START as usize)
				.wrapping_byte_add(addr)
				.wrapping_byte_add(descr::NORMAL);

			entry_i += 1;
		}

		table_i += 1;
	}

	tables
};

pub unsafe fn init(uart_address: u32) {
	unsafe {
		LEVEL_3_TABLE_SERIAL.0[1] = ptr::with_exposed_provenance_mut::<()>(uart_address as usize)
			.wrapping_byte_add(descr::NON_CACHEABLE);
	}
}

pub unsafe fn enable() {
	// Set Translation Table Base Registers (TTBR)
	TTBR1_EL1.set(0);
	TTBR0_EL1.set((&raw mut LEVEL_0_TABLE).expose_provenance() as u64);
	dsb(SY);
	isb(SY);

	// Set MMU enable in System Control Register (SCTLR)
	SCTLR_EL1.modify(SCTLR_EL1::M::Enable);
	isb(SY);

	info!("Successfully set up paging.");
}

#[derive(Clone, Copy, Debug)]
#[repr(C, align(0x1000))]
struct PageTable([*mut (); 512]);

/// Descriptor values
///
/// For reference, see <https://developer.arm.com/documentation/ddi0487/mb/-Part-D-The-AArch64-System-Level-Architecture/-Chapter-D8-The-AArch64-Virtual-Memory-System-Architecture/-D8-3-Translation-table-descriptor-formats/-D8-3-1-VMSAv8-64-descriptor-formats>.
mod descr {
	pub const NORMAL: usize = AF | SH_INNER | attr_indx(4) | TABLE | VALID;
	pub const NON_CACHEABLE: usize = AF | SH_INNER | attr_indx(3) | TABLE | VALID;

	/// Valid descriptor
	const VALID: usize = 1;

	/// Table descriptor
	const TABLE: usize = 1 << 1;

	/// Attribute index
	///
	/// Selects the corresponding `MAIR` memory region attributes.
	const fn attr_indx(indx: u8) -> usize {
		assert!(indx < 1 << 5);
		(indx as usize) << 2
	}

	/// Shareability
	///
	/// Inner Shareable
	const SH_INNER: usize = 1 << 8 | 1 << 9;

	/// Access flag
	const AF: usize = 1 << 10;

	/// A software-defined marker for marking a self-referential entry.
	///
	/// This can be used for recursive page tables by the kernel, but is currently not needed.
	// FIXME: remove once the kernel set's up it's own page tables.
	pub const SELF: usize = 1 << 55;
}
