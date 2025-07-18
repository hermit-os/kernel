use core::arch::asm;
use core::mem::MaybeUninit;

pub(crate) struct SerialDevice {
	pub addr: u32,
}

impl SerialDevice {
	pub fn new() -> Self {
		let base = crate::env::boot_info()
			.hardware_info
			.serial_port_base
			.map(|uartport| uartport.get())
			.unwrap();

		Self { addr: base as u32 }
	}

	pub fn write(&self, buf: &[u8]) {
		let port = core::ptr::with_exposed_provenance_mut::<u8>(self.addr as usize);
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

	pub fn read(&self, _buf: &mut [MaybeUninit<u8>]) -> crate::io::Result<usize> {
		Ok(0)
	}

	pub fn can_read(&self) -> bool {
		false
	}
}
