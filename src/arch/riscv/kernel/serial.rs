// TODO: sifive UART
use crate::arch::riscv::kernel::sbi;

pub struct SerialPort {}

impl SerialPort {
	pub const fn new(port_address: u32) -> Self {
		Self {}
	}

	pub fn write_byte(&self, byte: u8) {
		// LF newline characters need to be extended to CRLF over a real serial port.
		if byte == b'\n' {
			sbi::console_putchar('\r' as usize);
		}

		sbi::console_putchar(byte as usize);
	}

	pub fn init(&self, _baudrate: u32) {
		// We don't do anything here (yet).
	}
}
