use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::mem::MaybeUninit;

use hermit_sync::{InterruptTicketMutex, Lazy};

#[cfg(feature = "pci")]
use crate::arch::x86_64::kernel::interrupts;
#[cfg(feature = "pci")]
use crate::drivers::InterruptLine;

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

	pub fn write(&self, buf: &[u8]) {
		let mut guard = UART_DEVICE.lock();

		for &data in buf {
			guard.uart.send(data);
		}
	}

	pub fn read(&self, buf: &mut [MaybeUninit<u8>]) -> crate::io::Result<usize> {
		let mut guard = UART_DEVICE.lock();
		if guard.buffer.is_empty() {
			Ok(0)
		} else {
			let min = core::cmp::min(buf.len(), guard.buffer.len());
			let drained = guard.buffer.drain(..min).collect::<Vec<_>>();
			buf[..min].write_copy_of_slice(drained.as_slice());
			Ok(min)
		}
	}

	pub fn can_read(&self) -> bool {
		!UART_DEVICE.lock().buffer.is_empty()
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
