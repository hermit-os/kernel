use alloc::collections::VecDeque;
use core::hint;

use embedded_io::{ErrorType, Read, ReadReady, Write};
use hermit_sync::{InterruptTicketMutex, Lazy};
use uart_16550::backend::PioBackend;
use uart_16550::{Config, Uart16550};

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
		uart.init(Config::default()).unwrap();
		uart.test_loopback().unwrap();
		uart.check_remote_ready_to_receive().unwrap();

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

		loop {
			match guard.uart.try_send_bytes(buf) {
				Ok(n) => return Ok(n),
				Err(_) => hint::spin_loop(),
			}
		}
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

#[cfg(feature = "pci")]
pub(crate) fn get_serial_handler() -> (InterruptLine, fn()) {
	fn serial_handler() {
		let mut guard = UART_DEVICE.lock();

		while let Ok(byte) = guard.uart.try_receive_byte() {
			guard.buffer.push_back(byte);
		}

		drop(guard);
		crate::console::CONSOLE_WAKER.lock().wake();
	}

	interrupts::add_irq_name(SERIAL_IRQ, "COM1");

	(SERIAL_IRQ, serial_handler)
}
