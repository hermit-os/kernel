use core::mem;
use core::ptr;
use multiboot::information::Multiboot;
use x86::controlregs;
use x86::irq::PageFaultError;
use x86_64::structures::paging::{
	Mapper, Page, PhysFrame, RecursivePageTable, Size1GiB, Size2MiB, Size4KiB,
};

#[cfg(feature = "smp")]
use crate::arch::x86_64::kernel::apic;
use crate::arch::x86_64::kernel::get_mbinfo;
use crate::arch::x86_64::kernel::irq;
use crate::arch::x86_64::kernel::processor;
use crate::arch::x86_64::mm::physicalmem;
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr, MEM};
use crate::env;
use crate::mm;
use crate::scheduler;

/// Pointer to the root page table (PML4)
const PML4_ADDRESS: VirtAddr = VirtAddr(0xFFFF_FFFF_FFFF_F000);

/// Number of Offset bits of a virtual address for a 4 KiB page, which are shifted away to get its Page Frame Number (PFN).
const PAGE_BITS: usize = 12;

/// Number of bits of the index in each table (PML4, PDPT, PD, PT).
const PAGE_MAP_BITS: usize = 9;

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

pub extern "x86-interrupt" fn page_fault_handler(
	stack_frame: irq::ExceptionStackFrame,
	error_code: u64,
) {
	let virtual_address = unsafe { controlregs::cr2() };

	// Anything else is an error!
	let pferror = PageFaultError::from_bits_truncate(error_code as u32);
	error!("Page Fault (#PF) Exception: {:#?}", stack_frame);
	error!(
		"virtual_address = {:#X}, page fault error = {}",
		virtual_address, pferror
	);
	error!(
		"fs = {:#X}, gs = {:#X}",
		processor::readfs(),
		processor::readgs()
	);

	// clear cr2 to signalize that the pagefault is solved by the pagefault handler
	unsafe {
		controlregs::cr2_write(0);
	}

	scheduler::abort();
}

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
///             The PRESENT, ACCESSED, and DIRTY flags are already set automatically.
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

unsafe fn level_4_table() -> &'static mut x86_64::structures::paging::PageTable {
	unsafe { &mut *(PML4_ADDRESS.as_mut_ptr() as *mut x86_64::structures::paging::PageTable) }
}

unsafe fn recursive_page_table() -> RecursivePageTable<'static> {
	unsafe { RecursivePageTable::new(level_4_table()).unwrap() }
}

fn map_page<S: PageSize>(page: Page<S>, phys_addr: PhysAddr, flags: PageTableEntryFlags) -> bool {
	use x86_64::{PhysAddr, VirtAddr};

	trace!(
		"Mapping {:p} to {phys_addr:p} ({}) with {flags:?}",
		page.start_address(),
		S::SIZE
	);

	let flags = flags
		| PageTableEntryFlags::PRESENT
		| PageTableEntryFlags::ACCESSED
		| PageTableEntryFlags::DIRTY;

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

pub fn identity_map(start_address: PhysAddr, end_address: PhysAddr) {
	let first_page =
		Page::<BasePageSize>::containing_address(x86_64::VirtAddr::new(start_address.as_u64()));
	let last_page =
		Page::<BasePageSize>::containing_address(x86_64::VirtAddr::new(end_address.as_u64()));
	assert!(
		last_page.start_address().as_u64() < mm::kernel_start_address().0,
		"Address {:#X} to be identity-mapped is not below Kernel start address",
		last_page.start_address()
	);

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().read_only().execute_disable();
	let count = (last_page.start_address().as_u64() - first_page.start_address().as_u64())
		/ BasePageSize::SIZE
		+ 1;
	map::<BasePageSize>(
		VirtAddr(first_page.start_address().as_u64()),
		PhysAddr(first_page.start_address().as_u64()),
		count as usize,
		flags,
	);
}

#[inline]
pub fn get_application_page_size() -> usize {
	LargePageSize::SIZE as usize
}

pub fn init() {}

pub fn init_page_tables() {
	debug!("Create new view to the kernel space");

	unsafe {
		let pml4 = controlregs::cr3();
		let pde = pml4 + 2 * BasePageSize::SIZE;

		debug!("Found PML4 at {:#x}", pml4);

		// make sure that only the required areas are mapped
		let start = pde
			+ ((mm::kernel_end_address().as_usize() >> (PAGE_MAP_BITS + PAGE_BITS))
				* mem::size_of::<u64>()) as u64;
		let size = (512 - (mm::kernel_end_address().as_usize() >> (PAGE_MAP_BITS + PAGE_BITS)))
			* mem::size_of::<u64>();

		ptr::write_bytes(start as *mut u8, 0u8, size);

		//TODO: clearing the memory before kernel_start_address()

		// flush tlb
		controlregs::cr3_write(pml4);

		// Identity-map the supplied Multiboot information and command line.
		let mb_info = get_mbinfo();
		if !mb_info.is_zero() {
			info!("Found Multiboot info at {:#x}", mb_info);
			identity_map(PhysAddr(mb_info.as_u64()), PhysAddr(mb_info.as_u64()));

			// Map the "Memory Map" information too.
			let mb = Multiboot::from_ptr(mb_info.as_u64(), &mut MEM).unwrap();
			let memory_map_address = mb
				.memory_regions()
				.expect("Could not find a memory map in the Multiboot information")
				.next()
				.expect("Could not first map address")
				.base_address();
			identity_map(PhysAddr(memory_map_address), PhysAddr(memory_map_address));
		}

		let cmdsize = env::get_cmdsize();
		if cmdsize > 0 {
			let cmdline = env::get_cmdline();
			info!("Found cmdline at {:#x} (size {})", cmdline, cmdsize);
			identity_map(
				PhysAddr(cmdline.as_u64()),
				PhysAddr(cmdline.as_u64()) + cmdsize - 1u64,
			);
		}
	}
}
