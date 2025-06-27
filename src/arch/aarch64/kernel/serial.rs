use core::arch::asm;

#[cfg(all(feature = "pci", feature = "console"))]
use crate::drivers::pci::get_console_driver;
#[cfg(all(not(feature = "pci"), feature = "console"))]
use crate::kernel::mmio::get_console_driver;
use crate::syscalls::interfaces::serial_buf_hypercall;

enum SerialInner {
	None,
	Uart(u32),
	Uhyve,
	#[cfg(feature = "console")]
	Virtio,
}

pub struct SerialPort {
	inner: SerialInner,
}

impl SerialPort {
	pub fn new(port_address: Option<u64>) -> Self {
		if crate::env::is_uhyve() {
			Self {
				inner: SerialInner::Uhyve,
			}
		} else if let Some(port_address) = port_address {
			Self {
				inner: SerialInner::Uart(port_address.try_into().unwrap()),
			}
		} else {
			Self {
				inner: SerialInner::None,
			}
		}
	}

	#[cfg(feature = "console")]
	pub fn switch_to_virtio_console(&mut self) {
		self.inner = SerialInner::Virtio;
	}

	pub fn write_buf(&mut self, buf: &[u8]) {
		match &mut self.inner {
			SerialInner::None => {
				// No serial port configured, do nothing.
			}
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
			#[cfg(feature = "console")]
			SerialInner::Virtio => {
				if let Some(console_driver) = get_console_driver() {
					let _ = console_driver.lock().write(buf);
				}
			}
		}
	}

	pub fn init(&self, _baudrate: u32) {
		// We don't do anything here (yet).
	}
}
