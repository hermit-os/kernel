#![allow(dead_code)]

use core::{fmt, mem};

use embedded_io::{ErrorType, Read, ReadReady, Write};
use heapless::Vec;
use hermit_sync::{InterruptTicketMutex, Lazy};

use crate::arch::SerialDevice;
#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioUART;
use crate::errno::Errno;
use crate::executor::WakerRegistration;
#[cfg(not(target_arch = "riscv64"))]
use crate::syscalls::interfaces::serial_buf_hypercall;

const SERIAL_BUFFER_SIZE: usize = 256;

pub(crate) enum IoDevice {
	#[cfg(not(target_arch = "riscv64"))]
	Uhyve(UhyveSerial),
	Uart(SerialDevice),
	#[cfg(feature = "virtio-console")]
	Virtio(VirtioUART),
}

impl ErrorType for IoDevice {
	type Error = Errno;
}

impl Read for IoDevice {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		match self {
			#[cfg(not(target_arch = "riscv64"))]
			IoDevice::Uhyve(s) => s.read(buf),
			IoDevice::Uart(s) => s.read(buf),
			#[cfg(feature = "virtio-console")]
			IoDevice::Virtio(s) => s.read(buf),
		}
	}
}

impl ReadReady for IoDevice {
	fn read_ready(&mut self) -> Result<bool, Self::Error> {
		match self {
			#[cfg(not(target_arch = "riscv64"))]
			IoDevice::Uhyve(s) => s.read_ready(),
			IoDevice::Uart(s) => s.read_ready(),
			#[cfg(feature = "virtio-console")]
			IoDevice::Virtio(s) => s.read_ready(),
		}
	}
}

impl Write for IoDevice {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		match self {
			#[cfg(not(target_arch = "riscv64"))]
			IoDevice::Uhyve(s) => s.write_all(buf)?,
			IoDevice::Uart(s) => s.write_all(buf)?,
			#[cfg(feature = "virtio-console")]
			IoDevice::Virtio(s) => s.write_all(buf)?,
		};

		#[cfg(all(target_arch = "x86_64", feature = "vga"))]
		for &byte in buf {
			// vga::write_byte() checks if VGA support has been initialized,
			// so we don't need any additional if clause around it.
			crate::arch::kernel::vga::write_byte(byte);
		}

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

#[cfg(not(target_arch = "riscv64"))]
pub(crate) struct UhyveSerial;

#[cfg(not(target_arch = "riscv64"))]
impl UhyveSerial {
	pub const fn new() -> Self {
		Self {}
	}
}

#[cfg(not(target_arch = "riscv64"))]
impl ErrorType for UhyveSerial {
	type Error = Errno;
}

#[cfg(not(target_arch = "riscv64"))]
impl Read for UhyveSerial {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		let _ = buf;
		Ok(0)
	}
}

#[cfg(not(target_arch = "riscv64"))]
impl ReadReady for UhyveSerial {
	fn read_ready(&mut self) -> Result<bool, Self::Error> {
		Ok(false)
	}
}

#[cfg(not(target_arch = "riscv64"))]
impl Write for UhyveSerial {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		serial_buf_hypercall(buf);
		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

pub(crate) struct Console {
	device: IoDevice,
	buffer: Vec<u8, SERIAL_BUFFER_SIZE>,
}

impl Console {
	pub fn new(device: IoDevice) -> Self {
		Self {
			device,
			buffer: Vec::new(),
		}
	}

	#[cfg(feature = "virtio-console")]
	pub fn replace_device(&mut self, device: IoDevice) {
		self.device = device;
	}
}

impl ErrorType for Console {
	type Error = Errno;
}

impl Read for Console {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		self.device.read(buf)
	}
}

impl ReadReady for Console {
	fn read_ready(&mut self) -> Result<bool, Self::Error> {
		self.device.read_ready()
	}
}

impl Write for Console {
	/// Writes a buffer to the console.
	/// The content is buffered until a newline is encountered or the internal buffer is full.
	/// To force early output, use [`flush`](Self::flush).
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		if SERIAL_BUFFER_SIZE - self.buffer.len() >= buf.len() {
			// unwrap: we checked that buf fits in self.buffer
			self.buffer.extend_from_slice(buf).unwrap();
			if buf.contains(&b'\n') {
				self.flush()?;
			}
		} else {
			self.device.write_all(&self.buffer)?;
			self.buffer.clear();
			if buf.len() >= SERIAL_BUFFER_SIZE {
				self.device.write_all(buf)?;
			} else {
				// unwrap: we checked that buf fits in self.buffer
				self.buffer.extend_from_slice(buf).unwrap();
				if buf.contains(&b'\n') {
					self.flush()?;
				}
			}
		}

		Ok(buf.len())
	}

	/// Immediately writes everything in the internal buffer to the output.
	fn flush(&mut self) -> Result<(), Self::Error> {
		if !self.buffer.is_empty() {
			self.device.write_all(&self.buffer)?;
			self.buffer.clear();
		}
		Ok(())
	}
}

pub(crate) static CONSOLE_WAKER: InterruptTicketMutex<WakerRegistration> =
	InterruptTicketMutex::new(WakerRegistration::new());
pub(crate) static CONSOLE: Lazy<InterruptTicketMutex<Console>> = Lazy::new(|| {
	crate::CoreLocal::install();

	#[cfg(not(target_arch = "riscv64"))]
	if crate::env::is_uhyve() {
		InterruptTicketMutex::new(Console::new(IoDevice::Uhyve(UhyveSerial::new())))
	} else {
		InterruptTicketMutex::new(Console::new(IoDevice::Uart(SerialDevice::new())))
	}
	#[cfg(target_arch = "riscv64")]
	InterruptTicketMutex::new(Console::new(IoDevice::Uart(SerialDevice::new())))
});

#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
	CONSOLE.lock().write_fmt(args).unwrap();
}

#[doc(hidden)]
pub fn _panic_print(args: fmt::Arguments<'_>) {
	let mut console = unsafe { CONSOLE.make_guard_unchecked() };
	console.write_fmt(args).ok();
	mem::forget(console);
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	#[test]
	fn test_console() {
		println!("HelloWorld");
	}
}
