use core::arch::asm;

use crate::syscalls::interfaces::serial_buf_hypercall;

enum SerialInner {
	Uart(u32),
	Uhyve,
}

pub struct SerialPort {
	inner: SerialInner,
}

impl SerialPort {
	pub fn new(port_address: u32) -> Self {
		if crate::env::is_uhyve() {
			Self {
				inner: SerialInner::Uhyve,
			}
		} else {
			Self {
				inner: SerialInner::Uart(port_address),
			}
		}
	}

	pub fn write_buf(&mut self, buf: &[u8]) {
		match &mut self.inner {
			SerialInner::Uhyve => {
				serial_buf_hypercall(buf);
			}
			SerialInner::Uart(port_address) => {
				let port = core::ptr::with_exposed_provenance_mut::<u8>(*port_address as usize);
				for &byte in buf {
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
			}
		}
	}

	pub fn init(&self, _baudrate: u32) {
		// We don't do anything here (yet).
	}
}
