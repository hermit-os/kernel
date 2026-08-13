//! FIXME(mkroening): upstream these

#![cfg_attr(not(feature = "hermit-entry"), expect(dead_code))]

use align_address::Align;
use free_list::{PageRange, PageRangeError};

pub trait PageRangeExt: Sized {
	fn containing(start: usize, end: usize) -> Result<Self, PageRangeError>;

	fn and(self, rhs: Self) -> Option<Self>;
}

impl PageRangeExt for PageRange {
	fn containing(start: usize, end: usize) -> Result<Self, PageRangeError> {
		let start = start.align_down(free_list::PAGE_SIZE);
		let end = end.align_up(free_list::PAGE_SIZE);
		Self::new(start, end)
	}

	fn and(self, rhs: Self) -> Option<Self> {
		let start = self.start().max(rhs.start());
		let end = self.end().min(rhs.end());
		Self::new(start, end).ok()
	}
}
