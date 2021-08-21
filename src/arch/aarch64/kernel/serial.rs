// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use core::ptr;

pub struct SerialPort {
	port_address: u32,
}

impl SerialPort {
	pub const fn new(port_address: u32) -> Self {
		Self {
			port_address: port_address,
		}
	}

	pub fn write_byte(&self, byte: u8) {
		let port = self.port_address as *mut u8;

		// LF newline characters need to be extended to CRLF over a real serial port.
		if byte == b'\n' {
			unsafe {
				core::ptr::write_volatile(port, b'\r');
			}
		}

		unsafe {
			core::ptr::write_volatile(port, byte);
		}
	}

	pub fn init(&self, baudrate: u32) {
		// We don't do anything here (yet).
	}
}
