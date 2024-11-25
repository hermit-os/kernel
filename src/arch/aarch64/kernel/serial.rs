use core::arch::asm;

use heapless::Vec;

use crate::syscalls::interfaces::serial_buf_hypercall;

const SERIAL_BUFFER_SIZE: usize = 256;

#[allow(clippy::large_enum_variant)]
enum SerialInner {
	Uart(u32),
	Uhyve(Vec<u8, SERIAL_BUFFER_SIZE>), // heapless vec to have print before allocators are initialized
}

pub struct SerialPort {
	inner: SerialInner,
}

impl SerialPort {
	pub fn new(port_address: u32) -> Self {
		if crate::env::is_uhyve() {
			Self {
				inner: SerialInner::Uhyve(Vec::new()),
			}
		} else {
			Self {
				inner: SerialInner::Uart(port_address),
			}
		}
	}

	pub fn write_buf(&mut self, buf: &[u8]) {
		match &mut self.inner {
			SerialInner::Uhyve(output_buf) => {
				if SERIAL_BUFFER_SIZE - output_buf.len() >= buf.len() {
					// unwrap: we checked that buf fits in output_buf
					output_buf.extend_from_slice(buf).unwrap();
					if buf.contains(&b'\n') {
						serial_buf_hypercall(output_buf);
						output_buf.clear();
					}
				} else {
					serial_buf_hypercall(output_buf);
					output_buf.clear();
					if buf.len() >= SERIAL_BUFFER_SIZE {
						serial_buf_hypercall(buf);
					} else {
						// unwrap: we checked that buf fits in output_buf
						output_buf.extend_from_slice(buf).unwrap();
						if buf.contains(&b'\n') {
							serial_buf_hypercall(output_buf);
							output_buf.clear();
						}
					}
				}
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

	#[allow(dead_code)]
	pub fn read(&mut self) -> Option<u8> {
		None
	}

	pub fn init(&self, _baudrate: u32) {
		// We don't do anything here (yet).
	}
}
