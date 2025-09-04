use alloc::collections::vec_deque::VecDeque;
use core::ptr::NonNull;

use arm_pl011_uart::{DataBits, Interrupts, LineConfig, Parity, StopBits, Uart, UniqueMmioPointer};
use embedded_io::{ErrorType, Read, ReadReady, Write};
use hermit_sync::{InterruptTicketMutex, Lazy};

use crate::errno::Errno;

static UART_DEVICE: Lazy<InterruptTicketMutex<UartDevice>> =
	Lazy::new(|| InterruptTicketMutex::new(UartDevice::new()));

pub(crate) struct UartDevice {
	uart: Uart<'static>,
	buffer: VecDeque<u8>,
}

impl UartDevice {
	pub fn new() -> Self {
		let base = crate::env::boot_info()
			.hardware_info
			.serial_port_base
			.map(|uartport| uartport.get())
			.unwrap();

		let uart_pointer =
			unsafe { UniqueMmioPointer::new(NonNull::new_unchecked(base as *mut _)) };

		let mut uart = Uart::new(uart_pointer);

		let line_config = LineConfig {
			data_bits: DataBits::Bits8,
			parity: Parity::None,
			stop_bits: StopBits::One,
		};
		uart.enable(line_config, 115_200, 16_000_000).unwrap();

		uart.set_interrupt_masks(Interrupts::RXI | Interrupts::RTI);
		uart.clear_interrupts(Interrupts::all());

		Self {
			uart,
			buffer: VecDeque::new(),
		}
	}
}

pub(crate) struct SerialDevice;

impl SerialDevice {
	pub fn new() -> Self {
		Self
	}
}

impl ErrorType for SerialDevice {
	type Error = Errno;
}

impl Read for SerialDevice {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		let mut guard = UART_DEVICE.lock();

		if guard.buffer.is_empty() {
			Ok(0)
		} else {
			let min = buf.len().min(guard.buffer.len());

			for (dst, src) in buf[..min].iter_mut().zip(guard.buffer.drain(..min)) {
				*dst = src;
			}

			Ok(min)
		}
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

		for byte in buf {
			guard.uart.write_word(*byte);
		}

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

pub(crate) fn handle_uart_interrupt() {
	let mut guard = UART_DEVICE.lock();

	while let Ok(Some(mut byte)) = guard.uart.read_word() {
		// Normalize CR to LF
		if byte == b'\r' {
			byte = b'\n';
		}

		guard.buffer.push_back(byte);
	}

	guard
		.uart
		.clear_interrupts(Interrupts::RXI | Interrupts::RTI);

	drop(guard);

	crate::console::CONSOLE_WAKER.lock().wake();
}
