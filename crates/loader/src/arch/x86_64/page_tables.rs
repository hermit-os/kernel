//! Page Tables.
//!
//! This module defines the page tables that we switch to by setting `CR3` to `LEVEL_4_TABLE`.
//!
//! Specifically, we map the first GiB of virtual memory using 512 2-MiB pages.
//! 2-MiB pages are supported on every x86-64 CPU.
//!
//! # Alternatives
//!
//! Filling these structs in assembly at runtime would likely be faster, but also less readable.
//!
//! # Current implementation
//!
//! Some page tables need to point to other page tables, but also contain flags.
//! We do this by adding the flags as bytes to the pointer, which is possible in const-eval.
//! The resulting expression is relocatable.
//!
//! Casting pointers to integers is not possible in const-eval.
//! Asserting that all flag bits in the address are 0 is not possible.
//! Using a bitwise OR (`|`) operation cannot be expressed and would not be relocatable.
//!
//! For details, see this discussion: [rust-lang/rust#51910 (comment)].
//!
//! [rust-lang/rust#51910 (comment)]: https://github.com/rust-lang/rust/issues/51910#issuecomment-1013271838

use core::ptr;

use x86_64::structures::paging::{PageSize, PageTableFlags, Size2MiB};

const TABLE_FLAGS: PageTableFlags = PageTableFlags::PRESENT.union(PageTableFlags::WRITABLE);
const PAGE_FLAGS: PageTableFlags = TABLE_FLAGS.union(PageTableFlags::HUGE_PAGE);

pub static mut LEVEL_4_TABLE: PageTable = {
	let flags = TABLE_FLAGS.bits() as usize;

	let mut page_table = [ptr::null_mut(); _];

	page_table[0] = (&raw mut LEVEL_3_TABLE).wrapping_byte_add(flags).cast();

	PageTable(page_table)
};

static mut LEVEL_3_TABLE: PageTable = {
	let flags = TABLE_FLAGS.bits() as usize;

	let mut page_table = [ptr::null_mut(); _];

	page_table[0] = (&raw mut LEVEL_2_TABLE).wrapping_byte_add(flags).cast();

	PageTable(page_table)
};

static mut LEVEL_2_TABLE: PageTable = {
	let flags: usize = PAGE_FLAGS.bits() as usize;

	let mut page_table = [ptr::null_mut(); _];

	let mut i = 0;
	while i < page_table.len() {
		let addr = i * Size2MiB::SIZE as usize;
		page_table[i] = ptr::with_exposed_provenance_mut(addr + flags);
		i += 1;
	}

	PageTable(page_table)
};

#[repr(align(0x1000))]
#[repr(C)]
pub struct PageTable([*mut (); 512]);
