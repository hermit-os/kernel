use core::num::NonZeroU32;
use core::ptr::NonNull;

use volatile::{VolatileFieldAccess, VolatileRef};

use crate::arch::drivers::SerialSuccess::Success;
use crate::arch::drivers::{SerialDriver, SerialSuccess};

#[repr(C)]
#[derive(VolatileFieldAccess)]
struct QemuPort {
	out: u8,
}

pub struct QemuSerial {
	regs: VolatileRef<'static, QemuPort>,
}

impl QemuSerial {
	pub fn from_addr(base_addr: NonZeroU32) -> QemuSerial {
		Self {
			regs: unsafe {
				VolatileRef::new(NonNull::new_unchecked(base_addr.get() as *mut QemuPort))
			},
		}
	}
}

impl SerialDriver for QemuSerial {
	fn init(&mut self) {}
	fn putc(&mut self, c: u8) -> SerialSuccess<u8> {
		self.regs.as_mut_ptr().out().write(c);
		Success(c)
	}

	fn putstr(&mut self, s: &[u8]) {
		for c in s.iter().copied() {
			let _ = self.putc(c);
		}
	}
	fn get_addr(&self) -> u32 {
		self.regs.as_ptr().as_raw_ptr().as_ptr() as u32
	}

	fn wait_empty(&mut self) {}
}
