use alloc::collections::VecDeque;

use embedded_io::{ErrorType, Read, ReadReady, Write};
use hermit_sync::{InterruptTicketMutex, Lazy};

#[cfg(feature = "pci")]
use crate::arch::x86_64::kernel::interrupts;
#[cfg(feature = "pci")]
use crate::drivers::InterruptLine;
use crate::errno::Errno;

#[cfg(feature = "pci")]
const SERIAL_IRQ: u8 = 4;

static UART_DEVICE: Lazy<InterruptTicketMutex<UartDevice>> =
	Lazy::new(|| unsafe { InterruptTicketMutex::new(UartDevice::new()) });

struct UartDevice {
	pub uart: uart_16550::SerialPort,
	pub buffer: VecDeque<u8>,
}

impl UartDevice {
	pub unsafe fn new() -> Self {
		let base = crate::env::boot_info()
			.hardware_info
			.serial_port_base
			.unwrap()
			.get();
		let mut uart = unsafe { uart_16550::SerialPort::new(base) };
		uart.init();

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
		Ok(UART_DEVICE.lock().buffer.read_ready()?)
	}
}

impl Write for SerialDevice {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		let mut guard = UART_DEVICE.lock();

		for &data in buf {
			guard.uart.send(data);
		}

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

#[cfg(feature = "pci")]
pub(crate) fn get_serial_handler() -> (InterruptLine, fn()) {
	fn serial_handler() {
		let mut guard = UART_DEVICE.lock();
		if let Ok(c) = guard.uart.try_receive() {
			guard.buffer.push_back(c);
		}

		drop(guard);
		crate::console::CONSOLE_WAKER.lock().wake();
	}

	interrupts::add_irq_name(SERIAL_IRQ, "COM1");

	(SERIAL_IRQ, serial_handler)
}
