use uart_16550::SerialPort;

pub struct Console {
	serial_port: SerialPort,
}

impl Console {
	pub fn write_bytes(&mut self, bytes: &[u8]) {
		for byte in bytes.iter().copied() {
			self.serial_port.send(byte);
		}
	}
}

impl Default for Console {
	fn default() -> Self {
		let mut serial_port = unsafe { SerialPort::new(0x3F8) };
		serial_port.init();
		Self { serial_port }
	}
}
