use x86_64::instructions::port::Port;

use crate::arch::x86_64::kernel::interrupts::{self, IDT};
use crate::arch::x86_64::kernel::{apic, COM1};

enum Inner {
	Uart(uart_16550::SerialPort),
	Uhyve(Port<u8>),
}

pub struct SerialPort(Inner);

impl SerialPort {
	pub unsafe fn new(base: u16) -> Self {
		if crate::env::is_uhyve() {
			let serial = Port::new(base);
			Self(Inner::Uhyve(serial))
		} else {
			let mut serial = unsafe { uart_16550::SerialPort::new(base) };
			serial.init();
			Self(Inner::Uart(serial))
		}
	}

	pub fn receive(&mut self) -> u8 {
		if let Inner::Uart(s) = &mut self.0 {
			s.receive()
		} else {
			0
		}
	}

	pub fn send(&mut self, buf: &[u8]) {
		match &mut self.0 {
			Inner::Uhyve(s) => {
				for &data in buf {
					unsafe {
						s.write(data);
					}
				}
			}
			Inner::Uart(s) => {
				for &data in buf {
					s.send(data);
				}
			}
		}
	}
}

extern "x86-interrupt" fn serial_interrupt(_stack_frame: crate::interrupts::ExceptionStackFrame) {
	let c = unsafe { char::from_u32_unchecked(COM1.lock().as_mut().unwrap().receive().into()) };
	if !c.is_ascii_control() {
		print!("{}", c);
	}

	apic::eoi();
}

pub(crate) fn install_serial_interrupt() {
	const SERIAL_IRQ: usize = 36;

	unsafe {
		let mut idt = IDT.lock();
		idt[SERIAL_IRQ]
			.set_handler_fn(serial_interrupt)
			.set_stack_index(0);
	}
	interrupts::add_irq_name((SERIAL_IRQ - 32).try_into().unwrap(), "COM1");
}
