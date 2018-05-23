// Copyright (c) 2017 Colin Finck, RWTH Aachen University
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

use core::sync::atomic::spin_loop_hint;
use environment;
use x86::shared::io::*;

const UART_TX: u16 = 0;
const UART_IER: u16 = 1;

const UART_DLL: u16 = 0;
const UART_DLM: u16 = 1;

const UART_FCR: u16 = 2;
const UART_FCR_ENABLE_FIFO:            u8 = 0x01;
const UART_FCR_CLEAR_RECEIVER_FIFO:    u8 = 0x02;
const UART_FCR_CLEAR_TRANSMITTER_FIFO: u8 = 0x04;

const UART_LCR: u16 = 3;
const UART_LCR_WORD_LENGTH_8BITS:    u8 = 0x03;
const UART_LCR_DIVISOR_LATCH_ACCESS: u8 = 0x80;

const UART_LSR: u16 = 5;
const UART_LSR_EMPTY_TRANSMITTER_HOLDING_REGISTER: u8 = 0x20;


pub struct SerialPort {
	port_address: u16
}

impl SerialPort {
	pub const fn new(port_address: u16) -> Self {
		Self { port_address: port_address }
	}

	fn read_from_register(&self, register: u16) -> u8 {
		unsafe { inb(self.port_address + register) }
	}

	fn is_transmitting(&self) -> bool {
		// The virtual serial port in uhyve is never blocked.
		if environment::is_uhyve() {
			return false;
		}

		(self.read_from_register(UART_LSR) & UART_LSR_EMPTY_TRANSMITTER_HOLDING_REGISTER == 0)
	}

	fn write_to_register(&self, register: u16, byte: u8) {
		while self.is_transmitting() {
			spin_loop_hint();
		}

		unsafe { outb(self.port_address + register, byte); }
	}

	pub fn write_byte(&self, byte: u8) {
		// LF newline characters need to be extended to CRLF over a real serial port.
		if byte == b'\n' {
			self.write_to_register(UART_TX, b'\r');
		}

		self.write_to_register(UART_TX, byte);
	}

	pub fn init(&self, baudrate: u32) {
		// The virtual serial port is always initialized in uhyve.
		if environment::is_uhyve() {
			return;
		}

		// Disable port interrupt.
		self.write_to_register(UART_IER, 0);

		// Set 8N1 mode (8 bits, 1 stop bit, no parity).
		self.write_to_register(UART_LCR, UART_LCR_WORD_LENGTH_8BITS);

		// Set the baudrate.
		let divisor = (115200 / baudrate) as u16;
		let lcr = self.read_from_register(UART_LCR);
		self.write_to_register(UART_LCR, lcr | UART_LCR_DIVISOR_LATCH_ACCESS);
		self.write_to_register(UART_DLL, divisor as u8);
		self.write_to_register(UART_DLM, (divisor >> 8) as u8);
		self.write_to_register(UART_LCR, lcr);

		// Enable and clear FIFOs.
		self.write_to_register(UART_FCR, UART_FCR_ENABLE_FIFO | UART_FCR_CLEAR_RECEIVER_FIFO | UART_FCR_CLEAR_TRANSMITTER_FIFO);
	}
}
