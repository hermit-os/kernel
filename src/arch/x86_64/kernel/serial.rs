#[cfg(feature = "shell")]
use alloc::collections::VecDeque;

use heapless::Vec;

use crate::arch::x86_64::kernel::core_local::increment_irq_counter;
use crate::arch::x86_64::kernel::interrupts::{self, IDT};
use crate::arch::x86_64::kernel::{apic, COM1};
use crate::syscalls::interfaces::serial_buf_hypercall;

const SERIAL_IRQ: u8 = 36;

const SERIAL_BUFFER_SIZE: usize = 256;

#[allow(clippy::large_enum_variant)]
enum SerialInner {
	Uart(uart_16550::SerialPort),
	Uhyve(Vec<u8, SERIAL_BUFFER_SIZE>), // heapless vec to have print before allocators are initialized
}

pub struct SerialPort {
	inner: SerialInner,
	#[cfg(feature = "shell")]
	buffer: VecDeque<u8>,
}

impl SerialPort {
	pub unsafe fn new(base: u16) -> Self {
		if crate::env::is_uhyve() {
			Self {
				inner: SerialInner::Uhyve(Vec::new()),
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
			SerialInner::Uhyve(output_buf) => {
				if SERIAL_BUFFER_SIZE - output_buf.len() >= buf.len() {
					// unwrap: we checked that buf fits in output_buf
					output_buf.extend_from_slice(buf).unwrap();
					if buf.contains(&b'\n') {
						serial_buf_hypercall(output_buf);
						output_buf.clear();
					}
				} else {
					serial_buf_hypercall(output_buf);
					output_buf.clear();
					if buf.len() >= SERIAL_BUFFER_SIZE {
						serial_buf_hypercall(buf);
					} else {
						// unwrap: we checked that buf fits in output_buf
						output_buf.extend_from_slice(buf).unwrap();
						if buf.contains(&b'\n') {
							serial_buf_hypercall(output_buf);
							output_buf.clear();
						}
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
	COM1.lock().as_mut().unwrap().buffer_input();
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
