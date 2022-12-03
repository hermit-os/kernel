use x86_64::instructions::port::Port;

enum Inner {
	Uart(uart_16550::SerialPort),
	Uhyve(Port<u8>),
}

pub struct SerialPort(Inner);

impl SerialPort {
	pub unsafe fn new(base: u16) -> Self {
		if crate::env::is_uhyve() {
			let serial = Port::new(base);
			Self(Inner::Uhyve(serial))
		} else {
			let mut serial = unsafe { uart_16550::SerialPort::new(base) };
			serial.init();
			Self(Inner::Uart(serial))
		}
	}

	pub fn send(&mut self, data: u8) {
		match &mut self.0 {
			Inner::Uhyve(s) => unsafe { s.write(data) },
			Inner::Uart(s) => s.send(data),
		}
	}
}
