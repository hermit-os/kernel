use alloc::collections::vec_deque::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use hermit_sync::SpinMutex;
use smoltcp::phy;
use smoltcp::time::Instant;

use crate::drivers::net::NetworkDriver;
use crate::drivers::{Driver, InterruptLine};
use crate::mm::device_alloc::DeviceAlloc;

pub(crate) struct LoopbackDriver {
	queue: SpinMutex<VecDeque<Vec<u8, DeviceAlloc>>>,
	reserved_receives: AtomicUsize,
}

impl LoopbackDriver {
	pub(crate) const fn new() -> Self {
		Self {
			queue: SpinMutex::new(VecDeque::new()),
			reserved_receives: AtomicUsize::new(0),
		}
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

pub(crate) struct TxToken<'a> {
	queue: &'a SpinMutex<VecDeque<Vec<u8, DeviceAlloc>>>,
}

impl smoltcp::phy::TxToken for TxToken<'_> {
	fn consume<R, F>(self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		let mut buffer = Vec::with_capacity_in(len, DeviceAlloc);
		buffer.resize(len, 0);
		let result = f(&mut buffer);
		self.queue.lock().push_back(buffer);
		result
	}
}

pub(crate) struct RxToken<'a> {
	queue: &'a SpinMutex<VecDeque<Vec<u8, DeviceAlloc>>>,
	reserved_receives: &'a AtomicUsize,
}

impl smoltcp::phy::RxToken for RxToken<'_> {
	fn consume<R, F>(self, f: F) -> R
	where
		F: FnOnce(&[u8]) -> R,
	{
		let frame = self.queue.lock().pop_front();
		f(&frame.unwrap())
	}
}

impl Drop for RxToken<'_> {
	fn drop(&mut self) {
		self.reserved_receives.fetch_sub(1, Ordering::Relaxed);
	}
}

impl smoltcp::phy::Device for LoopbackDriver {
	type RxToken<'a> = RxToken<'a>;
	type TxToken<'a> = TxToken<'a>;

	fn receive(&mut self, _: Instant) -> Option<(RxToken<'_>, TxToken<'_>)> {
		if self.queue.lock().len() <= self.reserved_receives.load(Ordering::Relaxed) {
			return None;
		}

		self.reserved_receives.fetch_add(1, Ordering::Relaxed);
		Some((
			RxToken {
				queue: &self.queue,
				reserved_receives: &self.reserved_receives,
			},
			TxToken { queue: &self.queue },
		))
	}

	fn transmit(&mut self, _: Instant) -> Option<TxToken<'_>> {
		Some(TxToken { queue: &self.queue })
	}

	fn capabilities(&self) -> phy::DeviceCapabilities {
		let mut capabilities = phy::DeviceCapabilities::default();
		capabilities.medium = phy::Medium::Ethernet;
		// Technically Linux uses 2^16, which we cannot use until we switch
		// to u32 for MTU
		capabilities.max_transmission_unit = u16::MAX.into();
		capabilities
	}
}

impl NetworkDriver for LoopbackDriver {
	fn get_mac_address(&self) -> [u8; 6] {
		// This matches Linux' behavior
		[0; 6]
	}

	fn has_packet(&self) -> bool {
		!self.queue.lock().is_empty()
	}

	fn set_polling_mode(&mut self, _value: bool) {
		// no-op
	}

	fn handle_interrupt(&mut self) {
		// no-op
	}
}

pub(crate) type NetworkDevice = LoopbackDriver;
