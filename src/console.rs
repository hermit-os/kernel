#![allow(dead_code)]

use core::mem::MaybeUninit;
use core::{fmt, mem};

use heapless::Vec;
use hermit_sync::{InterruptTicketMutex, Lazy};

use crate::arch::SerialDevice;
#[cfg(feature = "console")]
use crate::drivers::console::VirtioUART;
use crate::executor::WakerRegistration;
use crate::io;
#[cfg(not(target_arch = "riscv64"))]
use crate::syscalls::interfaces::serial_buf_hypercall;

const SERIAL_BUFFER_SIZE: usize = 256;

pub(crate) enum IoDevice {
	#[cfg(not(target_arch = "riscv64"))]
	Uhyve(UhyveSerial),
	Uart(SerialDevice),
	#[cfg(feature = "console")]
	Virtio(VirtioUART),
}

impl IoDevice {
	pub fn write(&self, buf: &[u8]) {
		match self {
			#[cfg(not(target_arch = "riscv64"))]
			IoDevice::Uhyve(s) => s.write(buf),
			IoDevice::Uart(s) => s.write(buf),
			#[cfg(feature = "console")]
			IoDevice::Virtio(s) => s.write(buf),
		}

		#[cfg(all(target_arch = "x86_64", feature = "vga"))]
		for &byte in buf {
			// vga::write_byte() checks if VGA support has been initialized,
			// so we don't need any additional if clause around it.
			crate::arch::kernel::vga::write_byte(byte);
		}
	}

	pub fn read(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		match self {
			#[cfg(not(target_arch = "riscv64"))]
			IoDevice::Uhyve(s) => s.read(buf),
			IoDevice::Uart(s) => s.read(buf),
			#[cfg(feature = "console")]
			IoDevice::Virtio(s) => s.read(buf),
		}
	}

	pub fn can_read(&self) -> bool {
		match self {
			#[cfg(not(target_arch = "riscv64"))]
			IoDevice::Uhyve(s) => s.can_read(),
			IoDevice::Uart(s) => s.can_read(),
			#[cfg(feature = "console")]
			IoDevice::Virtio(s) => s.can_read(),
		}
	}
}

#[cfg(not(target_arch = "riscv64"))]
pub(crate) struct UhyveSerial;

#[cfg(not(target_arch = "riscv64"))]
impl UhyveSerial {
	pub const fn new() -> Self {
		Self {}
	}

	pub fn write(&self, buf: &[u8]) {
		serial_buf_hypercall(buf);
	}

	pub fn read(&self, _buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		Ok(0)
	}

	pub fn can_read(&self) -> bool {
		false
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

	/// Writes a buffer to the console.
	/// The content is buffered until a newline is encountered or the internal buffer is full.
	/// To force early output, use [`flush`](Self::flush).
	pub fn write(&mut self, buf: &[u8]) {
		if SERIAL_BUFFER_SIZE - self.buffer.len() >= buf.len() {
			// unwrap: we checked that buf fits in self.buffer
			self.buffer.extend_from_slice(buf).unwrap();
			if buf.contains(&b'\n') {
				self.flush();
			}
		} else {
			self.device.write(&self.buffer);
			self.buffer.clear();
			if buf.len() >= SERIAL_BUFFER_SIZE {
				self.device.write(buf);
			} else {
				// unwrap: we checked that buf fits in self.buffer
				self.buffer.extend_from_slice(buf).unwrap();
				if buf.contains(&b'\n') {
					self.flush();
				}
			}
		}
	}

	/// Immediately writes everything in the internal buffer to the output.
	pub fn flush(&mut self) {
		if !self.buffer.is_empty() {
			self.device.write(&self.buffer);
			self.buffer.clear();
		}
	}

	pub fn read(&mut self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		self.device.read(buf)
	}

	pub fn can_read(&self) -> bool {
		self.device.can_read()
	}

	#[cfg(feature = "console")]
	pub fn replace_device(&mut self, device: IoDevice) {
		self.device = device;
	}
}

/// A collection of methods that are required to format
/// a message to Hermit's console.
impl fmt::Write for Console {
	/// Print a string of characters.
	#[inline]
	fn write_str(&mut self, s: &str) -> fmt::Result {
		if !s.is_empty() {
			self.write(s.as_bytes());
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
	use fmt::Write;
	CONSOLE.lock().write_fmt(args).unwrap();
}

#[doc(hidden)]
pub fn _panic_print(args: fmt::Arguments<'_>) {
	use fmt::Write;
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
