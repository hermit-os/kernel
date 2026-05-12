use alloc::collections::VecDeque;
use core::hint;

use embedded_io::{ErrorType, Read, ReadReady, Write};
use hermit_sync::{InterruptTicketMutex, Lazy};
use uart_16550::backend::PioBackend;
use uart_16550::{Config, Uart16550};

#[cfg(feature = "pci")]
use crate::drivers::InterruptHandlerMap;
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
		let base_port = 0x3f8;
		let mut uart = unsafe { Uart16550::new_port(base_port).unwrap() };
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
		let uart = &mut guard.uart;

		for byte in buf.iter().copied() {
			match byte {
				// backspace or delete
				8 | 0x7f => {
					uart.try_send_byte(8).unwrap();
					uart.try_send_byte(b' ').unwrap();
					uart.try_send_byte(8).unwrap();
				}
				// Normal Rust newlines to terminal-compatible newlines.
				b'\n' => {
					uart.try_send_byte(b'\r').unwrap();
					uart.try_send_byte(b'\n').unwrap();
				}
				data => {
					uart.try_send_byte(data).unwrap();
				}
			}
		}

		Ok(buf.len())
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

#[cfg(feature = "pci")]
pub(crate) fn register_handler(handlers: &mut InterruptHandlerMap) {
	super::interrupts::add_irq_name(SERIAL_IRQ, "COM1");
	handlers
		.entry(SERIAL_IRQ)
		.or_default()
		.push_back(handle_interrupt);
}
