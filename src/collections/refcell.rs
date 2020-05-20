// Copyright (c) 2020 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A shareable mutable interrupt-safe memory location with
//! dynamically checked borrow rules
//!
//! In principle it embeds `RefCell<T>` from the core library into a new object.
//! By borrowing the reference, interrupts are automatically disabled to support
//! the usage within an interrupt handler.

use crate::arch::kernel::irq;
use core::cell;
use core::ops::{Deref, DerefMut};

pub struct Ref<'b, T: 'b> {
	value: cell::Ref<'b, T>,
	irq: bool,
}

impl<'b, T: 'b> Ref<'b, T> {
	pub fn new(value: cell::Ref<'b, T>, irq: bool) -> Self {
		Self {
			value: value,
			irq: irq,
		}
	}
}

impl<T> Deref for Ref<'_, T> {
	type Target = T;

	#[inline]
	fn deref(&self) -> &T {
		&*(self.value)
	}
}

impl<T> Drop for Ref<'_, T> {
	#[inline]
	fn drop(&mut self) {
		irq::nested_enable(self.irq);
	}
}

pub struct RefMut<'b, T: 'b> {
	value: cell::RefMut<'b, T>,
	irq: bool,
}

impl<'b, T: 'b> RefMut<'b, T> {
	pub fn new(value: cell::RefMut<'b, T>, irq: bool) -> Self {
		Self {
			value: value,
			irq: irq,
		}
	}
}

impl<T> Deref for RefMut<'_, T> {
	type Target = T;

	#[inline]
	fn deref(&self) -> &T {
		&*self.value
	}
}

impl<T> DerefMut for RefMut<'_, T> {
	#[inline]
	fn deref_mut(&mut self) -> &mut T {
		&mut *self.value
	}
}

impl<T> Drop for RefMut<'_, T> {
	#[inline]
	fn drop(&mut self) {
		irq::nested_enable(self.irq);
	}
}

/// A mutable interrupt-safe memory location with
/// dynamically checked borrow rules
pub struct RefCell<T>(cell::RefCell<T>);

impl<T> RefCell<T> {
	#[inline]
	pub fn new(value: T) -> Self {
		Self(cell::RefCell::new(value))
	}

	#[inline]
	pub fn borrow(&self) -> Ref<'_, T> {
		self.try_borrow().expect("already mutably borrowed")
	}

	#[inline]
	pub fn try_borrow(&self) -> Result<Ref<'_, T>, cell::BorrowError> {
		let irq = irq::nested_disable();

		match self.0.try_borrow() {
			Ok(value) => Ok(Ref::new(value, irq)),
			Err(err) => {
				irq::nested_enable(irq);
				Err(err)
			}
		}
	}

	#[inline]
	pub fn borrow_mut(&self) -> RefMut<'_, T> {
		self.try_borrow_mut().expect("already borrowed")
	}

	#[inline]
	pub fn try_borrow_mut(&self) -> Result<RefMut<'_, T>, cell::BorrowMutError> {
		let irq = irq::nested_disable();

		match self.0.try_borrow_mut() {
			Ok(value) => Ok(RefMut::new(value, irq)),
			Err(err) => {
				irq::nested_enable(irq);
				Err(err)
			}
		}
	}
}
