pub(crate) mod paging;
pub(crate) mod physicalmem;
pub(crate) mod virtualmem;

use core::slice;

#[cfg(feature = "common-os")]
use align_address::Align;
pub use x86::bits64::paging::{PAddr as PhysAddr, VAddr as VirtAddr};
#[cfg(feature = "common-os")]
use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

pub use self::paging::init_page_tables;
#[cfg(feature = "common-os")]
use crate::arch::mm::paging::{PageTableEntryFlags, PageTableEntryFlagsExt};

/// Memory translation, allocation and deallocation for MultibootInformation
struct MultibootMemory;

impl multiboot::information::MemoryManagement for MultibootMemory {
	unsafe fn paddr_to_slice(
		&self,
		p: multiboot::information::PAddr,
		size: usize,
	) -> Option<&'static [u8]> {
		unsafe { Some(slice::from_raw_parts(p as _, size)) }
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

#[cfg(feature = "common-os")]
pub fn create_new_root_page_table() -> usize {
	let physaddr =
		physicalmem::allocate_aligned(BasePageSize::SIZE as usize, BasePageSize::SIZE as usize)
			.unwrap();
	let virtaddr =
		virtualmem::allocate_aligned(2 * BasePageSize::SIZE as usize, BasePageSize::SIZE as usize)
			.unwrap();
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();

	let entry: u64 = unsafe {
		let cr3 = x86::controlregs::cr3().align_down(BasePageSize::SIZE);
		paging::map::<BasePageSize>(virtaddr, PhysAddr(cr3), 1, flags);
		let entry: &u64 = &*virtaddr.as_ptr();

		*entry
	};

	let slice_addr = virtaddr + BasePageSize::SIZE;
	paging::map::<BasePageSize>(slice_addr, physaddr, 1, flags);

	unsafe {
		let pml4 = core::slice::from_raw_parts_mut(slice_addr.as_mut_ptr() as *mut u64, 512);

		// clear PML4
		for elem in pml4.iter_mut() {
			*elem = 0;
		}

		// copy first element and the self reference
		pml4[0] = entry;
		// create self reference
		pml4[511] = physaddr.as_u64() + 0x3; // PG_PRESENT | PG_RW
	};

	paging::unmap::<BasePageSize>(virtaddr, 2);
	virtualmem::deallocate(virtaddr, 2 * BasePageSize::SIZE as usize);

	physaddr.as_usize()
}

pub fn init() {
	paging::init();
	physicalmem::init();
	virtualmem::init();

	#[cfg(feature = "common-os")]
	unsafe {
		crate::scheduler::BOOT_ROOT_PAGE_TABLE
			.set(x86::controlregs::cr3().try_into().unwrap())
			.unwrap();
	}
}
