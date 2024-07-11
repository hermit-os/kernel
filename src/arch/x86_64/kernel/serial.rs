#[cfg(feature = "shell")]
use alloc::collections::VecDeque;

use x86_64::instructions::port::Port;

use crate::arch::x86_64::kernel::core_local::increment_irq_counter;
use crate::arch::x86_64::kernel::interrupts::{self, IDT};
use crate::arch::x86_64::kernel::{apic, COM1};
use crate::console::{self, SerialDevice};
use crate::io;

const SERIAL_IRQ: u8 = 36;

pub struct Uart {
	serial_port: uart_16550::SerialPort,
}

impl io::Read for Uart {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		for (i, byte) in buf.iter_mut().enumerate() {
			match self.serial_port.try_receive() {
				Ok(ok) => *byte = ok,
				Err(_) => return Ok(i),
			}
		}
		Ok(buf.len())
	}
}

impl io::Write for Uart {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		for (i, byte) in buf.iter().copied().enumerate() {
			match self.serial_port.try_send_raw(byte) {
				Ok(()) => {}
				Err(_) => return Ok(i),
			}
		}
		Ok(buf.len())
	}
}

impl SerialDevice for Uart {}

pub struct Uhyve {
	port: Port<u8>,
}

impl io::Read for Uhyve {
	fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
		Ok(0)
	}
}

impl io::Write for Uhyve {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		for byte in buf.iter().copied() {
			unsafe { self.port.write(byte) }
		}
		Ok(buf.len())
	}
}

mod dispatch {
	use alloc::string::String;
	use alloc::vec::Vec;

	use enum_dispatch::enum_dispatch;

	use super::{Uart, Uhyve};
	use crate::console::SerialDevice;
	use crate::io::{Read, Result, Write};
	use core::fmt;

	#[enum_dispatch(Read, Write, SerialDevice)]
	pub enum Serial {
		Uart,
		Uhyve,
	}
}

pub use dispatch::Serial;

enum SerialInner {
	Uart(uart_16550::SerialPort),
	Uhyve(Port<u8>),
}

pub struct SerialPort {
	inner: SerialInner,
	#[cfg(feature = "shell")]
	buffer: VecDeque<u8>,
}

impl SerialPort {
	pub unsafe fn new(base: u16) -> Self {
		if crate::env::is_uhyve() {
			let serial = Port::new(base);
			Self {
				inner: SerialInner::Uhyve(serial),
				#[cfg(feature = "shell")]
				buffer: VecDeque::new(),
			}
		} else {
			let mut serial = unsafe { uart_16550::SerialPort::new(base) };
			serial.init();
			Self {
				inner: SerialInner::Uart(serial),
				#[cfg(feature = "shell")]
				buffer: VecDeque::new(),
			}
		}
	}

	pub fn buffer_input(&mut self) {
		if let SerialInner::Uart(s) = &mut self.inner {
			let c = unsafe { char::from_u32_unchecked(s.receive().into()) };
			#[cfg(not(feature = "shell"))]
			if !c.is_ascii_control() {
				print!("{}", c);
			}
			#[cfg(feature = "shell")]
			self.buffer.push_back(c.try_into().unwrap());
		}
	}

	#[allow(dead_code)]
	#[cfg(feature = "shell")]
	pub fn read(&mut self) -> Option<u8> {
		self.buffer.pop_front()
	}

	#[allow(dead_code)]
	#[cfg(not(feature = "shell"))]
	pub fn read(&mut self) -> Option<u8> {
		None
	}

	pub fn send(&mut self, buf: &[u8]) {
		match &mut self.inner {
			SerialInner::Uhyve(s) => {
				for &data in buf {
					unsafe {
						s.write(data);
					}
				}
			}
			SerialInner::Uart(s) => {
				for &data in buf {
					s.send(data);
				}
			}
		}
	}
}

extern "x86-interrupt" fn serial_interrupt(_stack_frame: crate::interrupts::ExceptionStackFrame) {
	console::buffer_input();
	increment_irq_counter(SERIAL_IRQ);

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
