// TODO: Move this into its own crate.
#![allow(dead_code)]

use core::cmp::Ordering;
use core::fmt;
use core::ops::Range;

use align_address::Align;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddressRange {
	start: usize,
	end: usize,
}

impl AddressRange {
	pub fn new(start: usize, end: usize) -> Option<Self> {
		(start <= end).then_some(Self { start, end })
	}

	pub fn from_start_len(start: usize, len: usize) -> Self {
		Self {
			start,
			end: start + len,
		}
	}

	pub fn overlaps(self, other: Self) -> bool {
		self.partial_cmp(&other).is_none()
	}

	pub fn next(self, len: usize) -> Self {
		Self::from_start_len(self.end, len)
	}

	pub fn align_to(self, align: usize) -> Self {
		Self {
			start: self.start.align_down(align),
			end: self.end.align_up(align),
		}
	}

	pub fn start(self) -> usize {
		self.start
	}

	pub fn end(self) -> usize {
		self.end
	}

	pub fn len(self) -> usize {
		self.end - self.start
	}
}

#[derive(Debug)]
pub struct TryFromRangeError(());

impl<T> TryFrom<Range<*const T>> for AddressRange {
	type Error = TryFromRangeError;

	fn try_from(value: Range<*const T>) -> Result<Self, Self::Error> {
		Self::new(value.start as usize, value.end as usize).ok_or(TryFromRangeError(()))
	}
}

impl fmt::Display for AddressRange {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let Self { start, end } = self;
		let len = self.len();
		write!(f, "{start:#x}..{end:#x} (len = {len:#10x})")
	}
}

impl PartialOrd for AddressRange {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		if self.end <= other.start {
			Some(Ordering::Less)
		} else if self.start >= other.end {
			Some(Ordering::Greater)
		} else if self == other {
			Some(Ordering::Equal)
		} else {
			None
		}
	}
}
