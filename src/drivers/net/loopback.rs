use alloc::collections::vec_deque::VecDeque;
use alloc::vec::Vec;

use crate::drivers::net::NetworkDriver;
use crate::drivers::{Driver, InterruptLine};
use crate::executor::device::{RxToken, TxToken};
use crate::mm::device_alloc::DeviceAlloc;

pub(crate) struct LoopbackDriver(VecDeque<Vec<u8, DeviceAlloc>>);

impl LoopbackDriver {
	pub(crate) const fn new() -> Self {
		Self(VecDeque::new())
	}
}

impl Driver for LoopbackDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		// This is called by mmio / pci specific code, this driver
		// is using neither.
		unimplemented!()
	}

	fn get_name(&self) -> &'static str {
		"loopback"
	}
}

impl NetworkDriver for LoopbackDriver {
	fn get_mac_address(&self) -> [u8; 6] {
		// This matches Linux' behavior
		[0; 6]
	}

	fn get_mtu(&self) -> u16 {
		// Technically Linux uses 2^16, which we cannot use until we switch
		// to u32 for MTU
		u16::MAX
	}

	fn receive_packet(&mut self) -> Option<(RxToken, TxToken<'_>)> {
		self.0
			.pop_front()
			.map(move |buffer| (RxToken::new(buffer), TxToken::new(self)))
	}

	fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		let mut buffer = Vec::with_capacity_in(len, DeviceAlloc);
		buffer.resize(len, 0);
		let result = f(&mut buffer);
		self.0.push_back(buffer);
		result
	}

	fn has_packet(&self) -> bool {
		!self.0.is_empty()
	}

	fn set_polling_mode(&mut self, _value: bool) {
		// no-op
	}

	fn handle_interrupt(&mut self) {
		// no-op
	}
}

pub(crate) type NetworkDevice = LoopbackDriver;
