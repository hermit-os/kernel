use core::alloc::Layout;

use talc::base::Talc;
use talc::base::binning::Binning;
use talc::source::Source;

use crate::drivers::pci::get_balloon_driver;

/// [`Talc`] out of memory handler that attempts to recover memory previously
/// returned to the host via the VIRTIO Traditional Memory Balloon device.
///
/// It attempts to deflate the balloon (re-acquiring memory from the host, and
/// freeing the allocations made by the balloon driver in the host's stead) by
/// the amount required for the allocation that would have failed. If the balloon
/// is filled with fewer pages than would be required to cover the allocation's
/// size, this handler attempts to recover as many as possible still.
///
/// Memory freed across chunks of pages allocated for the balloon may not be
/// contiguous. This means that even if we free as many bytes as required for the
/// allocation, we may not have freed enough _contiguous_ memory for it. This is
/// ok however and [`Talc`] will simply call our handler again until we've either
/// exhausted the memory available for recovery from the host, or the allocation
/// succeeds.
#[derive(Debug)]
pub struct DeflateBalloonOnOom {
	/// Dummy field to prevent construction of the struct except through [`Self::new`]
	/// which is marked `unsafe`` and documents our requirements for safety.
	#[doc(hidden)]
	_private: (),
}

impl DeflateBalloonOnOom {
	/// Construct a new instance of the balloon deflating [`Source`] for [`Talc`].
	///
	/// # Safety
	/// May only be used with the one instance of [`Talc`] registered as Hermit's
	/// global allocator.
	pub const unsafe fn new() -> Self {
		Self { _private: () }
	}
}

// SAFETY: We only use the Talc reference passed in via the trait functions and
//         thus avoid deadlocks or panics. The balloon driver operations used in
//         this implementation are passed this reference to Talc and also don't
//         access the global Talc themselves. Hermit's logging implementation
//         (via the log::Log implementation on crate::logging::KernelLogger,
//         via hermit_macros::println!, via crate::console::_print)
//         does not allocate to while printing, so using such logging here is safe.
unsafe impl Source for DeflateBalloonOnOom {
	fn acquire<B: Binning>(talc: &mut Talc<Self, B>, layout: Layout) -> Result<(), ()> {
		warn!("<balloon:oom> Encountered OOM, attempting to deflate balloon to recover...");

		let Some(balloon_driver) = get_balloon_driver() else {
			return Err(());
		};

		let Some(mut ballon_driver_guard) = balloon_driver.try_lock() else {
			error!(
				"<balloon:oom> Driver was locked while attempting to allocate more than available. Unable to deflate balloon"
			);
			return Err(());
		};

		// For Talc's tag adjacent to the allocation, just always free one page more.
		// Divide rounding up so the allocation always fits even if it's not a multiple of 4K pages large.
		unsafe {
			ballon_driver_guard.deflate_for_oom(talc, (layout.size().div_ceil(4096)) as u32 + 1)
		}
	}
}
