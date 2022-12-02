pub mod paging;
pub mod physicalmem;
pub mod virtualmem;

use core::slice;

pub use x86::bits64::paging::{PAddr as PhysAddr, VAddr as VirtAddr};

pub use self::paging::init_page_tables;

/// Memory translation, allocation and deallocation for MultibootInformation
struct MultibootMemory;

impl multiboot::information::MemoryManagement for MultibootMemory {
	unsafe fn paddr_to_slice(
		&self,
		p: multiboot::information::PAddr,
		sz: usize,
	) -> Option<&'static [u8]> {
		unsafe { Some(slice::from_raw_parts(p as _, sz)) }
	}

	unsafe fn allocate(
		&mut self,
		_length: usize,
	) -> Option<(multiboot::information::PAddr, &mut [u8])> {
		None
	}

	unsafe fn deallocate(&mut self, addr: multiboot::information::PAddr) {
		if addr != 0 {
			unimplemented!()
		}
	}
}

pub fn init() {
	paging::init();
	physicalmem::init();
	virtualmem::init();
}
