use core::ptr;

use aarch64_cpu::asm::barrier::{SY, dsb, isb};
use aarch64_cpu::registers::{ReadWriteable, SCTLR_EL1, TTBR0_EL1, TTBR1_EL1, Writeable};
use log::info;

use super::RAM_START;
use super::paging::{BasePageSize, PageSize};

static mut LEVEL_0_TABLE: PageTable = PageTable([ptr::null_mut(); _]);
static mut LEVEL_1_TABLE: PageTable = PageTable([ptr::null_mut(); _]);
static mut LEVEL_2_TABLE_SERIAL: PageTable = PageTable([ptr::null_mut(); _]);
static mut LEVEL_2_TABLE_RAM: PageTable = PageTable([ptr::null_mut(); _]);
static mut LEVEL_3_TABLE_SERIAL: PageTable = PageTable([ptr::null_mut(); _]);
static mut LEVEL_3_TABLES_RAM: [PageTable; 10] = [PageTable([ptr::null_mut(); _]); 10];

#[allow(static_mut_refs)] // FIXME: disallow
pub unsafe fn init(uart_address: u32) {
	let level_0_table = unsafe { &mut LEVEL_0_TABLE.0 };
	level_0_table[0] = (&raw mut LEVEL_1_TABLE)
		.wrapping_byte_add(descr::NORMAL)
		.cast();
	level_0_table[511] = (&raw mut LEVEL_0_TABLE)
		.wrapping_byte_add(descr::NORMAL)
		.wrapping_byte_add(descr::SELF)
		.cast();

	let level_1_table = unsafe { &mut LEVEL_1_TABLE.0 };
	level_1_table[0] = (&raw mut LEVEL_2_TABLE_SERIAL)
		.wrapping_byte_add(descr::NORMAL)
		.cast();
	level_1_table[1] = (&raw mut LEVEL_2_TABLE_RAM)
		.wrapping_byte_add(descr::NORMAL)
		.cast();

	let level_2_table_serial = unsafe { &mut LEVEL_2_TABLE_SERIAL.0 };
	level_2_table_serial[0] = (&raw mut LEVEL_3_TABLE_SERIAL)
		.wrapping_byte_add(descr::NORMAL)
		.cast();

	let level_3_table_serial = unsafe { &mut LEVEL_3_TABLE_SERIAL.0 };
	level_3_table_serial[1] = ptr::with_exposed_provenance_mut::<()>(uart_address as usize)
		.wrapping_byte_add(descr::NON_CACHEABLE);

	let level_2_table_ram = unsafe { &mut LEVEL_2_TABLE_RAM.0 };

	for (i, level_3_table_ram) in unsafe { LEVEL_3_TABLES_RAM.iter_mut().enumerate() } {
		level_2_table_ram[i] = ptr::from_mut(level_3_table_ram)
			.wrapping_byte_add(descr::NORMAL)
			.cast();

		for (entry_i, entry) in level_3_table_ram.0.iter_mut().enumerate() {
			let addr = (i * 512 + entry_i) * BasePageSize::SIZE;
			*entry = ptr::with_exposed_provenance_mut::<()>(RAM_START as usize)
				.wrapping_byte_add(addr)
				.wrapping_byte_add(descr::NORMAL);
		}
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
