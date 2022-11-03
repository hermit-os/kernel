use core::ptr;

use x86_64::instructions::tlb;
use x86_64::structures::paging::{
	Mapper, Page, PageTableIndex, PhysFrame, RecursivePageTable, Size1GiB, Size2MiB, Size4KiB,
};

#[cfg(feature = "smp")]
use crate::arch::x86_64::kernel::apic;
use crate::arch::x86_64::mm::physicalmem;
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr};
use crate::env;
use crate::mm;

pub use x86_64::structures::paging::PageTableFlags as PageTableEntryFlags;

pub trait PageTableEntryFlagsExt {
	fn device(&mut self) -> &mut Self;

	fn normal(&mut self) -> &mut Self;

	fn read_only(&mut self) -> &mut Self;

	fn writable(&mut self) -> &mut Self;

	fn execute_disable(&mut self) -> &mut Self;
}

impl PageTableEntryFlagsExt for PageTableEntryFlags {
	fn device(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::NO_CACHE);
		self
	}

	fn normal(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::NO_CACHE);
		self
	}

	fn read_only(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::WRITABLE);
		self
	}

	fn writable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::WRITABLE);
		self
	}

	fn execute_disable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::NO_EXECUTE);
		self
	}
}

/// An entry in either table (PML4, PDPT, PD, PT)
#[derive(Clone, Copy)]
pub struct PageTableEntry {
	address: PhysAddr,
}

impl PageTableEntry {
	/// Return the stored physical address.
	pub fn address(self) -> PhysAddr {
		self.address
	}
}

pub use x86_64::structures::paging::PageSize;
pub use x86_64::structures::paging::Size1GiB as HugePageSize;
pub use x86_64::structures::paging::Size2MiB as LargePageSize;
pub use x86_64::structures::paging::Size4KiB as BasePageSize;

pub fn get_page_table_entry<S: PageSize>(virtual_address: VirtAddr) -> Option<PageTableEntry> {
	use x86_64::structures::paging::mapper::{MappedFrame, Translate, TranslateResult};

	trace!("Looking up Page Table Entry for {:#X}", virtual_address);

	let virtual_address = x86_64::VirtAddr::new(virtual_address.0);

	let frame = match unsafe { recursive_page_table().translate(virtual_address) } {
		TranslateResult::Mapped { frame, .. } => frame,
		TranslateResult::NotMapped => return None,
		TranslateResult::InvalidFrameAddress(_) => panic!(),
	};

	let start_address = match S::SIZE {
		Size4KiB::SIZE => match frame {
			MappedFrame::Size4KiB(frame) => frame.start_address(),
			_ => panic!(),
		},
		_ => panic!(),
	};

	let address = PhysAddr(start_address.as_u64());

	Some(PageTableEntry { address })
}

/// Translate a virtual memory address to a physical one.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> PhysAddr {
	use x86_64::structures::paging::mapper::Translate;

	let virtual_address = x86_64::VirtAddr::new(virtual_address.0);
	let phys_addr = unsafe {
		recursive_page_table()
			.translate_addr(virtual_address)
			.unwrap()
	};
	PhysAddr(phys_addr.as_u64())
}

#[no_mangle]
pub extern "C" fn virt_to_phys(virtual_address: VirtAddr) -> PhysAddr {
	virtual_to_physical(virtual_address)
}

/// Maps a continuous range of pages.
///
/// # Arguments
///
/// * `physical_address` - First physical address to map these pages to
/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or NO_EXECUTE).
///             The PRESENT flags is set automatically.
pub fn map<S: PageSize>(
	virtual_address: VirtAddr,
	physical_address: PhysAddr,
	count: usize,
	flags: PageTableEntryFlags,
) {
	trace!(
		"Mapping physical address {:#X} to virtual address {:#X} ({} pages)",
		physical_address,
		virtual_address,
		count
	);

	let first_page = Page::containing_address(x86_64::VirtAddr::new(virtual_address.0));
	let last_page = first_page + count as u64;
	let range = Page::range(first_page, last_page);

	let mut current_physical_address = physical_address;
	let mut send_ipi = false;

	for page in range {
		send_ipi |= map_page::<S>(page, current_physical_address, flags);
		current_physical_address += S::SIZE;
	}

	if send_ipi {
		#[cfg(feature = "smp")]
		apic::ipi_tlb_flush();
	}
}

unsafe fn recursive_page_table() -> RecursivePageTable<'static> {
	let level_4_table_addr = 0xFFFF_FFFF_FFFF_F000;
	let level_4_table_ptr = ptr::from_exposed_addr_mut(level_4_table_addr);
	unsafe {
		let level_4_table = &mut *(level_4_table_ptr);
		RecursivePageTable::new(level_4_table).unwrap()
	}
}

fn map_page<S: PageSize>(page: Page<S>, phys_addr: PhysAddr, flags: PageTableEntryFlags) -> bool {
	use x86_64::{PhysAddr, VirtAddr};

	trace!(
		"Mapping {:p} to {phys_addr:p} ({}) with {flags:?}",
		page.start_address(),
		S::SIZE
	);

	let flags = flags | PageTableEntryFlags::PRESENT;

	match S::SIZE {
		Size4KiB::SIZE => {
			let page =
				Page::<Size4KiB>::from_start_address(VirtAddr::new(page.start_address().as_u64()))
					.unwrap();
			let frame = PhysFrame::from_start_address(PhysAddr::new(phys_addr.0)).unwrap();
			unsafe {
				// TODO: Require explicit unmaps
				if let Ok((_frame, flush)) = recursive_page_table().unmap(page) {
					flush.flush();
				}
				recursive_page_table()
					.map_to(page, frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap()
					.flush();
			}
		}
		Size2MiB::SIZE => {
			let page =
				Page::<Size2MiB>::from_start_address(VirtAddr::new(page.start_address().as_u64()))
					.unwrap();
			let frame = PhysFrame::from_start_address(PhysAddr::new(phys_addr.0)).unwrap();
			unsafe {
				recursive_page_table()
					.map_to(page, frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap()
					.flush();
			}
		}
		Size1GiB::SIZE => {
			let page =
				Page::<Size1GiB>::from_start_address(VirtAddr::new(page.start_address().as_u64()))
					.unwrap();
			let frame = PhysFrame::from_start_address(PhysAddr::new(phys_addr.0)).unwrap();
			unsafe {
				recursive_page_table()
					.map_to(page, frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap()
					.flush();
			}
		}
		_ => unreachable!(),
	}

	true
}

pub fn unmap<S: PageSize>(virtual_address: VirtAddr, count: usize) {
	trace!(
		"Unmapping virtual address {:#X} ({} pages)",
		virtual_address,
		count
	);

	map::<S>(
		virtual_address,
		PhysAddr::zero(),
		count,
		PageTableEntryFlags::empty(),
	);
}

#[cfg(feature = "acpi")]
pub fn identity_map<S>(frame: PhysFrame<S>)
where
	S: PageSize + core::fmt::Debug,
	RecursivePageTable<'static>: Mapper<S>,
{
	assert!(
		frame.start_address().as_u64() < mm::kernel_start_address().0,
		"Address {:#X} to be identity-mapped is not below Kernel start address",
		frame.start_address()
	);

	unsafe {
		recursive_page_table()
			.identity_map(
				frame,
				PageTableEntryFlags::PRESENT | PageTableEntryFlags::NO_EXECUTE,
				&mut physicalmem::FrameAlloc,
			)
			.unwrap()
			.flush();
	}
}

#[inline]
pub fn get_application_page_size() -> usize {
	LargePageSize::SIZE as usize
}

pub fn init() {}

pub fn init_page_tables() {
	if env::is_uhyve() {
		// Uhyve identity-maps the first Gibibyte of memory (512 page table entries * 2MiB pages)
		// We now unmap all memory after the kernel image, so that we can remap it ourselves later for the heap.
		// Ideally, uhyve would only map as much memory as necessary, but this requires a hermit-entry ABI jump.
		// See https://github.com/hermitcore/uhyve/issues/426
		let kernel_end_addr = x86_64::VirtAddr::new(mm::kernel_end_address().as_u64());
		let start_page = Page::<Size2MiB>::from_start_address(kernel_end_addr).unwrap();
		let end_page = Page::from_page_table_indices_2mib(
			start_page.p4_index(),
			start_page.p3_index(),
			PageTableIndex::new(511),
		);
		let page_range = Page::range_inclusive(start_page, end_page);

		let mut page_table = unsafe { recursive_page_table() };
		for page in page_range {
			let (_frame, flush) = page_table.unmap(page).unwrap();
			flush.ignore();
		}

		tlb::flush_all();
	}
}
