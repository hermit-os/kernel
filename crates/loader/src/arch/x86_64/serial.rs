// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use core::hint::spin_loop;
use x86::io::*;

const UART_TX: u16 = 0;
const UART_IER: u16 = 1;

const UART_DLL: u16 = 0;
const UART_DLM: u16 = 1;

const UART_FCR: u16 = 2;
const UART_FCR_ENABLE_FIFO: u8 = 0x01;
const UART_FCR_CLEAR_RECEIVER_FIFO: u8 = 0x02;
const UART_FCR_CLEAR_TRANSMITTER_FIFO: u8 = 0x04;

const UART_LCR: u16 = 3;
const UART_LCR_WORD_LENGTH_8BITS: u8 = 0x03;
const UART_LCR_DIVISOR_LATCH_ACCESS: u8 = 0x80;

const UART_LSR: u16 = 5;
const UART_LSR_EMPTY_TRANSMITTER_HOLDING_REGISTER: u8 = 0x20;

pub struct SerialPort {
	port_address: u16,
}

impl SerialPort {
	pub const fn new(port_address: u16) -> Self {
		Self {
			port_address: port_address,
		}
	}

	fn read_from_register(&self, register: u16) -> u8 {
		unsafe { inb(self.port_address + register) }
	}

	fn is_transmitting(&self) -> bool {
		self.read_from_register(UART_LSR) & UART_LSR_EMPTY_TRANSMITTER_HOLDING_REGISTER == 0
	}

	fn write_to_register(&self, register: u16, byte: u8) {
		while self.is_transmitting() {
			spin_loop();
		}

		unsafe {
			outb(self.port_address + register, byte);
		}
	}

	pub fn write_byte(&self, byte: u8) {
		// LF newline characters need to be extended to CRLF over a real serial port.
		if byte == b'\n' {
			self.write_to_register(UART_TX, b'\r');
		}

		self.write_to_register(UART_TX, byte);
	}

	pub fn init(&self, baudrate: u32) {
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
		self.write_to_register(
			UART_FCR,
			UART_FCR_ENABLE_FIFO | UART_FCR_CLEAR_RECEIVER_FIFO | UART_FCR_CLEAR_TRANSMITTER_FIFO,
		);
	}
}
