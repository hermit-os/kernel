// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::irq;

mod cachepadded;
mod doublylinkedlist;

pub use self::cachepadded::*;
pub use self::doublylinkedlist::*;

/// `irqsave` guarantees that the call of the closure
/// will be not disturbed by an interrupt
#[inline]
pub fn irqsave<F: FnMut()>(mut f: F)
where
	F: FnOnce(),
{
	let irq = irq::nested_disable();
	f();
	irq::nested_enable(irq);
}

/// Help structure to disable interrupts as long as this data structure exists
pub struct AvoidInterrupts(bool);

impl AvoidInterrupts {
	#[inline]
	pub fn new() -> Self {
		Self(irq::nested_disable())
	}
}

impl Drop for AvoidInterrupts {
	#[inline]
	fn drop(&mut self) {
		irq::nested_enable(self.0);
	}
}
