// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use core::ptr;


pub struct SerialPort {
	port_address: u32
}

impl SerialPort {
	pub const fn new(port_address: u32) -> Self {
		Self { port_address: port_address }
	}

	pub fn write_byte(&self, byte: u8) {
		let port = self.port_address as *mut u8;

		// LF newline characters need to be extended to CRLF over a real serial port.
		if byte == b'\n' {
			unsafe { ptr::write_volatile(port, b'\r'); }
		}

		unsafe { ptr::write_volatile(port, byte); }
	}

	pub fn init(&self, baudrate: u32) {
		// We don't do anything here (yet).
	}
}
