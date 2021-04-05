// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;
use core::fmt;

pub struct Console;

/// A collection of methods that are required to format
/// a message to HermitCore's console.
impl fmt::Write for Console {
	/// Print a single character.
	#[cfg(feature = "aarch64-qemu-stdout")]
	fn write_char(&mut self, c: char) -> fmt::Result {
		unsafe {
			core::ptr::write_volatile(0x3F20_1000 as *mut u8, c as u8); //qemu raspi3
		}
		Ok(())
	}
	#[cfg(not(feature = "aarch64-qemu-stdout"))]
	fn write_char(&mut self, c: char) -> fmt::Result {
		arch::output_message_byte(c as u8);
		Ok(())
	}

	/// Print a string of characters.
	fn write_str(&mut self, s: &str) -> fmt::Result {
		for character in s.chars() {
			self.write_char(character).unwrap();
		}
		Ok(())
	}
}
