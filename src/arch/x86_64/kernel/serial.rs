use alloc::collections::VecDeque;
use core::task::Waker;

use crate::arch::x86_64::kernel::apic;
use crate::arch::x86_64::kernel::core_local::increment_irq_counter;
use crate::arch::x86_64::kernel::interrupts::{self, IDT};
#[cfg(all(feature = "pci", feature = "console"))]
use crate::drivers::pci::get_console_driver;
use crate::executor::WakerRegistration;
use crate::syscalls::interfaces::serial_buf_hypercall;

const SERIAL_IRQ: u8 = 36;

enum SerialInner {
	Uart(uart_16550::SerialPort),
	Uhyve,
	#[cfg(all(feature = "console", feature = "pci"))]
	Virtio,
}

pub struct SerialPort {
	inner: SerialInner,
	buffer: VecDeque<u8>,
	waker: WakerRegistration,
}

impl SerialPort {
	pub unsafe fn new(base: u16) -> Self {
		if crate::env::is_uhyve() {
			Self {
				inner: SerialInner::Uhyve,
				buffer: VecDeque::new(),
				waker: WakerRegistration::new(),
			}
		} else {
			let mut serial = unsafe { uart_16550::SerialPort::new(base) };
			serial.init();
			Self {
				inner: SerialInner::Uart(serial),
				buffer: VecDeque::new(),
				waker: WakerRegistration::new(),
			}
		}
	}

	pub fn buffer_input(&mut self) {
		if let SerialInner::Uart(s) = &mut self.inner {
			let c = s.receive();
			if c == b'\r' {
				self.buffer.push_back(b'\n');
			} else {
				self.buffer.push_back(c);
			}
			self.waker.wake();
		}
	}

	pub fn register_waker(&mut self, waker: &Waker) {
		self.waker.register(waker);
	}

	pub fn read(&mut self) -> Option<u8> {
		self.buffer.pop_front()
	}

	pub fn is_empty(&self) -> bool {
		self.buffer.is_empty()
	}

	pub fn send(&mut self, buf: &[u8]) {
		match &mut self.inner {
			SerialInner::Uhyve => serial_buf_hypercall(buf),
			SerialInner::Uart(s) => {
				for &data in buf {
					s.send(data);
				}
			}
			#[cfg(all(feature = "console", feature = "pci"))]
			SerialInner::Virtio => {
				if let Some(console_driver) = get_console_driver() {
					let _ = console_driver.lock().write(buf);
				}
			}
		}
	}

	#[cfg(all(feature = "pci", feature = "console"))]
	pub fn switch_to_virtio_console(&mut self) {
		self.inner = SerialInner::Virtio;
	}
}

extern "x86-interrupt" fn serial_interrupt(_stack_frame: crate::interrupts::ExceptionStackFrame) {
	crate::console::CONSOLE.lock().inner.buffer_input();
	increment_irq_counter(SERIAL_IRQ);
	crate::executor::run();

	apic::eoi();
}

pub(crate) fn install_serial_interrupt() {
	unsafe {
		let mut idt = IDT.lock();
		idt[SERIAL_IRQ]
			.set_handler_fn(serial_interrupt)
			.set_stack_index(0);
	}
	interrupts::add_irq_name(SERIAL_IRQ - 32, "COM1");
}
