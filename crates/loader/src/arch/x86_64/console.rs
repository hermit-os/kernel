use uart_16550::backend::PioBackend;
use uart_16550::{Config, Uart16550};

pub struct Console {
	uart: Uart16550<PioBackend>,
}

impl Console {
	pub fn write_bytes(&mut self, bytes: &[u8]) {
		self.uart.send_bytes_exact(bytes);
	}
}

impl Default for Console {
	fn default() -> Self {
		let base_port = 0x3f8;
		let mut uart = unsafe { Uart16550::new_port(base_port).unwrap() };
		uart.init(Config::default()).ok();
		// Once we have a fallback destination for output,
		// we should log any error above and run
		// `test_loopback` and `check_connected` here.

		Self { uart }
	}
}
