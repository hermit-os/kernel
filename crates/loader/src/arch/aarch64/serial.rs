pub struct SerialPort {
	port_address: u32,
}

impl SerialPort {
	pub const fn new(port_address: u32) -> Self {
		Self { port_address }
	}

	pub unsafe fn set_port(&mut self, addr: u32) {
		core::ptr::write_volatile(&mut self.port_address, addr);
	}

	pub unsafe fn get_port(&self) -> u32 {
		core::ptr::read_volatile(&self.port_address)
	}

	pub fn write_byte(&self, byte: u8) {
		unsafe {
			let port = core::ptr::read_volatile(&self.port_address) as *mut u8;

			// LF newline characters need to be extended to CRLF over a real serial port.
			if byte == b'\n' {
				core::ptr::write_volatile(port, b'\r');
			}

			core::ptr::write_volatile(port, byte);
		}
	}
}
