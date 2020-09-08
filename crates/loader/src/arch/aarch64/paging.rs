// Copyright (c) 2020 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use core::marker::PhantomData;

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
