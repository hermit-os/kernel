use core::arch::asm;

pub struct SerialPort {
	pub port_address: u32,
}

impl SerialPort {
	pub const fn new(port_address: u32) -> Self {
		Self { port_address }
	}

	pub fn write_byte(&self, byte: u8) {
		let port = core::ptr::with_exposed_provenance_mut::<u8>(self.port_address as usize);

		// LF newline characters need to be extended to CRLF over a real serial port.
		if byte == b'\n' {
			unsafe {
				asm!(
					"strb w8, [{port}]",
					port = in(reg) port,
					in("x8") b'\r',
					options(nostack),
				);
			}
		}

		unsafe {
			asm!(
				"strb w8, [{port}]",
				port = in(reg) port,
				in("x8") byte,
				options(nostack),
			);
		}
	}

	#[allow(dead_code)]
	pub fn read(&mut self) -> Option<u8> {
		None
	}

	pub fn init(&self, _baudrate: u32) {
		// We don't do anything here (yet).
	}
}
