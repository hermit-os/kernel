use alloc::collections::VecDeque;

use embedded_io::{ErrorType, Read, ReadReady, Write};
use hermit_sync::{InterruptTicketMutex, Lazy};
use uart_16550::backend::PioBackend;
use uart_16550::{Config, Uart16550};

use crate::errno::Errno;

#[cfg(feature = "pci")]
pub(crate) const SERIAL_IRQ: u8 = 4;

static UART_DEVICE: Lazy<InterruptTicketMutex<UartDevice>> =
	Lazy::new(|| unsafe { InterruptTicketMutex::new(UartDevice::new()) });

struct UartDevice {
	pub uart: Uart16550<PioBackend>,
	pub buffer: VecDeque<u8>,
}

impl UartDevice {
	pub unsafe fn new() -> Self {
		let base = crate::env::boot_info()
			.hardware_info
			.serial_port_base
			.unwrap()
			.get();
		let mut uart = unsafe { Uart16550::new_port(base).unwrap() };
		uart.init(Config::default()).ok();
		// Once we have a fallback destination for output,
		// we should log any error above and run
		// `test_loopback` and `check_connected` here.

		Self {
			uart,
			buffer: VecDeque::new(),
		}
	}
}

pub(crate) struct SerialDevice;

impl SerialDevice {
	pub fn new() -> Self {
		Self {}
	}
}

impl ErrorType for SerialDevice {
	type Error = Errno;
}

impl Read for SerialDevice {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		Ok(UART_DEVICE.lock().buffer.read(buf)?)
	}
}

impl ReadReady for SerialDevice {
	fn read_ready(&mut self) -> Result<bool, Self::Error> {
		Ok(!UART_DEVICE.lock().buffer.is_empty())
	}
}

impl Write for SerialDevice {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		let mut guard = UART_DEVICE.lock();
		let n = guard.uart.write(buf)?;
		Ok(n)
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

#[cfg(feature = "pci")]
pub(crate) fn handle_interrupt() {
	let mut guard = UART_DEVICE.lock();

	while guard.uart.read_ready().unwrap() {
		let mut buf = [0; 256];
		let n = guard.uart.read(&mut buf).unwrap();
		guard.buffer.write_all(&buf[..n]).unwrap();
	}

	drop(guard);
	crate::console::CONSOLE_WAKER.lock().wake();
}
