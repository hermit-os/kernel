#![cfg_attr(
	all(
		not(all(
			any(feature = "tcp", feature = "udp"),
			target_arch = "x86_64",
			feature = "rtl8139",
		)),
		not(all(
			any(feature = "tcp", feature = "udp"),
			target_arch = "riscv64",
			feature = "gem-net",
			not(feature = "pci")
		)),
		not(all(any(feature = "tcp", feature = "udp"), feature = "virtio-net")),
		not(feature = "vsock"),
		not(feature = "fuse"),
		not(feature = "console"),
	),
	expect(dead_code)
)]

use hermit_sync::{OnceCell, SpinMutex};

/// A cell for iteratively initializing a `OnceCell`.
///
/// This should be used as a stop-gap measure only.
pub struct InitCell<T> {
	init: SpinMutex<Option<T>>,
	once: OnceCell<T>,
}

impl<T> InitCell<T> {
	pub const fn new(val: T) -> Self {
		Self {
			init: SpinMutex::new(Some(val)),
			once: OnceCell::new(),
		}
	}

	pub fn with(&self, f: impl FnOnce(Option<&mut T>)) {
		let mut guard = self.init.lock();
		f((*guard).as_mut());
	}

	pub fn get(&self) -> Option<&T> {
		self.once.get()
	}

	pub fn finalize(&self) -> &T {
		self.once.get_or_init(|| self.init.lock().take().unwrap())
	}
}
