#![allow(dead_code)]

use core::marker::PhantomData;

/// Number of Offset bits of a virtual address for a 4 KiB page, which are shifted away to get its Page Frame Number (PFN).
pub const PAGE_BITS: usize = 12;

/// Number of bits of the index in each table (L0Table, L1Table, L2Table, L3Table).
pub const PAGE_MAP_BITS: usize = 9;

/// A mask where PAGE_MAP_BITS are set to calculate a table index.
pub const PAGE_MAP_MASK: usize = 0x1FF;

/// A generic interface to support all possible page sizes.
///
/// This is defined as a subtrait of Copy to enable #[derive(Clone, Copy)] for Page.
/// Currently, deriving implementations for these traits only works if all dependent types implement it as well.
pub trait PageSize: Copy {
	/// The page size in bytes.
	const SIZE: usize;
}

/// A 4 KiB page mapped in the PGT.
#[derive(Clone, Copy)]
pub enum BasePageSize {}
impl PageSize for BasePageSize {
	const SIZE: usize = 4096;
}

/// A 2 MiB page mapped in the PDT.
#[derive(Clone, Copy)]
pub enum LargePageSize {}
impl PageSize for LargePageSize {
	const SIZE: usize = 2 * 1024 * 1024;
}

/// A memory page of the size given by S.
#[derive(Clone, Copy)]
struct Page<S: PageSize> {
	/// Virtual memory address of this page.
	/// This is rounded to a page size boundary on creation.
	virtual_address: usize,

	/// Required by Rust to support the S parameter.
	size: PhantomData<S>,
}
