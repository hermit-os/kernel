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
}
