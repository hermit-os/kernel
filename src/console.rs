// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;
use crate::synch::spinlock::SpinlockIrqSave;
use core::fmt;

pub struct Console;

/// A collection of methods that are required to format
/// a message to HermitCore's console.
impl fmt::Write for Console {
	/// Print a string of characters.
	fn write_str(&mut self, s: &str) -> fmt::Result {
		if !s.is_empty() {
			let buf = s.as_bytes();
			arch::output_message_buf(buf);
		}

		Ok(())
	}

	/// Print a single character.
	fn write_char(&mut self, c: char) -> fmt::Result {
		self.write_str(c.encode_utf8(&mut [0; 4]))
	}
}

pub static CONSOLE: SpinlockIrqSave<Console> = SpinlockIrqSave::new(Console);

#[cfg(not(target_os = "hermit"))]
#[test]
fn test_console() {
	println!("HelloWorld");
}
