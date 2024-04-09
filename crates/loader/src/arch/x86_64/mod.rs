cfg_if::cfg_if! {
	if #[cfg(feature = "fc")] {
		mod firecracker;
		pub use self::firecracker::*;
	} else if #[cfg(target_os = "none")] {
		mod multiboot;
		pub use self::multiboot::*;
	}
}

mod console;
#[cfg(target_os = "none")]
mod paging;
#[cfg(target_os = "none")]
mod physicalmem;

pub use console::Console;

#[cfg(target_os = "none")]
const KERNEL_STACK_SIZE: u64 = 32_768;
#[cfg(target_os = "none")]
const SERIAL_IO_PORT: u16 = 0x3F8;

#[cfg(target_os = "none")]
unsafe fn map_memory(address: usize, memory_size: usize) -> usize {
	use align_address::Align;
	use x86_64::structures::paging::{PageSize, PageTableFlags, Size2MiB};

	let address = address.align_up(Size2MiB::SIZE as usize);
	let page_count = memory_size.align_up(Size2MiB::SIZE as usize) / Size2MiB::SIZE as usize;

	paging::map::<Size2MiB>(address, address, page_count, PageTableFlags::WRITABLE);

	address
}

#[cfg(target_os = "none")]
pub unsafe fn get_memory(memory_size: u64) -> u64 {
	use align_address::Align;
	use x86_64::structures::paging::{PageSize, Size2MiB};

	use self::physicalmem::PhysAlloc;

	let address = PhysAlloc::allocate((memory_size as usize).align_up(Size2MiB::SIZE as usize));
	unsafe { map_memory(address, memory_size as usize) as u64 }
}
