use alloc::collections::vec_deque::VecDeque;
use core::ptr::{self, NonNull};

use arm_pl011_uart::{
	DataBits, Interrupts, LineConfig, PL011Registers, Parity, StopBits, Uart, UniqueMmioPointer,
};
use embedded_io::{ErrorType, Read, ReadReady, Write};
use hermit_sync::{InterruptTicketMutex, Lazy};

use crate::env::{self, FdtStartInfo};
use crate::errno::Errno;

static UART_DEVICE: Lazy<InterruptTicketMutex<UartDevice>> =
	Lazy::new(|| InterruptTicketMutex::new(UartDevice::new()));

/// A 16550 compatible UART with byte-wide registers, as provided by Firecracker.
struct Ns16550 {
	base: NonNull<u8>,
}

// SAFETY: the registers are only accessed through `UART_DEVICE`.
unsafe impl Send for Ns16550 {}

impl Ns16550 {
	/// Receiver buffer and transmitter holding register
	const DATA: usize = 0;
	/// Interrupt enable register
	const IER: usize = 1;
	/// Line status register
	const LSR: usize = 5;

	const IER_RX_AVAILABLE: u8 = 1 << 0;
	const LSR_DATA_READY: u8 = 1 << 0;
	const LSR_THR_EMPTY: u8 = 1 << 5;

	fn read(&self, reg: usize) -> u8 {
		unsafe { self.base.add(reg).read_volatile() }
	}

	fn write(&mut self, reg: usize, value: u8) {
		unsafe { self.base.add(reg).write_volatile(value) }
	}

	fn write_byte(&mut self, byte: u8) {
		while self.read(Self::LSR) & Self::LSR_THR_EMPTY == 0 {}
		self.write(Self::DATA, byte);
	}

	fn read_byte(&mut self) -> Option<u8> {
		(self.read(Self::LSR) & Self::LSR_DATA_READY != 0).then(|| self.read(Self::DATA))
	}
}

enum Backend {
	Pl011(Uart<'static>),
	Ns16550(Ns16550),
}

pub(crate) struct UartDevice {
	backend: Backend,
	buffer: VecDeque<u8>,
}

impl UartDevice {
	pub fn new() -> Self {
		// The loader maps the serial port to 0x1000. Eventually, the kernel
		// should take care of the initial page tables. Until then, we hardcode
		// the value to slowly move away from the Hermit-specific boot info
		// struct.
		let is_ns16550 = env::start_info()
			.fdt()
			.is_some_and(|fdt| fdt.find_compatible(&["ns16550a"]).is_some());

		let backend = if is_ns16550 {
			let base = NonNull::new(ptr::with_exposed_provenance_mut::<u8>(0x1000)).unwrap();
			let mut uart = Ns16550 { base };
			uart.write(Ns16550::IER, Ns16550::IER_RX_AVAILABLE);
			Backend::Ns16550(uart)
		} else {
			let ptr = ptr::with_exposed_provenance_mut::<PL011Registers>(0x1000);
			let ptr = NonNull::new(ptr).unwrap();
			let ptr = unsafe { UniqueMmioPointer::new(ptr) };

			let mut uart = Uart::new(ptr);

			let line_config = LineConfig {
				data_bits: DataBits::Bits8,
				parity: Parity::None,
				stop_bits: StopBits::One,
			};
			uart.enable(line_config, 115_200, 16_000_000).unwrap();

			uart.set_interrupt_masks(Interrupts::RXI | Interrupts::RTI);
			uart.clear_interrupts(Interrupts::all());
			Backend::Pl011(uart)
		};

		Self {
			backend,
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

		for byte in buf {
			match &mut guard.backend {
				Backend::Pl011(uart) => uart.write_word(*byte),
				Backend::Ns16550(uart) => uart.write_byte(*byte),
			}
		}

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

pub(crate) fn handle_uart_interrupt() {
	let mut guard = UART_DEVICE.lock();
	let UartDevice { backend, buffer } = &mut *guard;

	// Normalize CR to LF
	let mut push = |byte: u8| buffer.push_back(if byte == b'\r' { b'\n' } else { byte });

	match backend {
		Backend::Pl011(uart) => {
			while let Ok(Some(byte)) = uart.read_word() {
				push(byte);
			}
			uart.clear_interrupts(Interrupts::RXI | Interrupts::RTI);
		}
		Backend::Ns16550(uart) => {
			while let Some(byte) = uart.read_byte() {
				push(byte);
			}
		}
	}

	drop(guard);

	crate::console::CONSOLE_WAKER.lock().wake();
}
