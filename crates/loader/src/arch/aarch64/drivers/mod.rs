use core::num::NonZeroU32;

use enum_dispatch::enum_dispatch;
use fdt::node::FdtNode;
use qemu_serial::QemuSerial;
use xlnx_serial::XlnxSerial;

pub mod qemu_serial;
pub mod xlnx_serial;

pub enum SerialSuccess<T> {
	Success(T),
	ERetry,
}

#[enum_dispatch]
pub trait SerialDriver {
	fn init(&mut self);
	fn putc(&mut self, c: u8) -> SerialSuccess<u8>;
	fn putstr(&mut self, s: &[u8]);
	fn get_addr(&self) -> u32;
	fn wait_empty(&mut self);
}

#[enum_dispatch(SerialDriver)]
pub enum SerialPort {
	Qemu(QemuSerial),
	Xlnx(XlnxSerial),
}

pub fn get_device<'a>(node: FdtNode<'_, 'a>) -> Option<SerialPort> {
	let compat = node.compatible()?;
	let reg = node.reg()?.next()?;

	for id in compat.all() {
		if id == "arm,pl011" {
			return Some(SerialPort::Qemu(QemuSerial::from_addr(
				NonZeroU32::new(reg.starting_address as u32).unwrap(),
			)));
		} else if id == "xlnx,xuartlite" {
			return Some(SerialPort::Xlnx(XlnxSerial::from_addr(
				NonZeroU32::new(reg.starting_address as u32).unwrap(),
			)));
		}
	}
	None
}
