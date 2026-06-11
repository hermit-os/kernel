use core::fmt::{Display, Formatter};
use core::ops::Add;

use align_address::Align;
use free_list::{FreeList, PageLayout, PageRange};
use hermit_sync::{InterruptTicketMutex, Lazy};
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::mm::paging;
#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{BasePageSize, HugePageSize, PageSize, PageTableEntryFlags};
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator, virtualmem};

static MAX_STACK_SIZE: usize = HugePageSize::SIZE as usize;
/// End of the stack. Ideally, we'd take the heap end, but in x86_64 this causes ptr computation
/// problems
static STACK_REGION_END: usize = virtualmem::kernel_heap_end().as_usize() + 1 - MAX_STACK_SIZE;
static STACK_REGION_START: usize = STACK_REGION_END - MAX_STACK_SIZE;
static STACK_FREE_LIST: Lazy<InterruptTicketMutex<FreeList<16>>> = Lazy::new(|| {
	// Remove all mappings in the stack region range
	let start = VirtAddr::new(STACK_REGION_START as u64);
	let count = MAX_STACK_SIZE / HugePageSize::SIZE as usize;
	let range = PageRange::new(STACK_REGION_START, STACK_REGION_END).unwrap();

	// Take the pages from the allocator
	PageAlloc::allocate_at(range).expect("failed to reserve stack area");

	paging::unmap::<HugePageSize>(start, count);

	let mut free_list = FreeList::new();
	unsafe {
		free_list
			.deallocate(PageRange::new(STACK_REGION_START, STACK_REGION_END).unwrap())
			.expect("failed to deallocate stack range");
	}

	InterruptTicketMutex::new(free_list)
});

const _: () = {
	assert!(MAX_STACK_SIZE.is_multiple_of(HugePageSize::SIZE as usize));
};

/// Size of the debug marker at the very top of each stack.
///
/// We have a marker at the very top of the stack for debugging (`0xdeadbeef`), which should not be overridden.
pub const MARKER_SIZE: usize = 0x10;
pub const MARKER: u64 = 0xdead_beef;
pub const GUARD_PAGE_MARKER: u64 = 0xdead_cafe;

pub fn allocate_stack(requested_size: usize) -> StackAllocation {
	// Determine basic number of pages
	let num_pages =
		requested_size.align_up(BasePageSize::SIZE as usize) / BasePageSize::SIZE as usize;

	let pages_with_guard = num_pages + 1;
	let size = pages_with_guard * BasePageSize::SIZE as usize;
	let layout = PageLayout::from_size_align(size, BasePageSize::SIZE as usize).unwrap();

	// Allocate virtual memory for this stack
	let page_range = STACK_FREE_LIST
		.lock()
		.allocate(layout)
		.expect("failed to allocate virtual memory space for stack");
	let stack_start = page_range.start();

	// Allocate physical pages for this stack
	let frame_range = FrameAlloc::allocate(layout).expect("failed to allocate frames for stack");
	let phys_addr_start = PhysAddr::new(frame_range.start() as u64);
	let virt_addr_start = VirtAddr::new(stack_start as u64);

	// Map first page to a disabled page full of a marker, then unmap it
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().execute_disable();
	paging::map::<BasePageSize>(virt_addr_start, phys_addr_start, 1, flags);
	unsafe {
		let marker_pos = virt_addr_start.add(BasePageSize::SIZE as usize - size_of::<u64>());
		*marker_pos.as_mut_ptr::<u64>() = GUARD_PAGE_MARKER;
	}
	paging::unmap::<BasePageSize>(virt_addr_start, 1);

	// Map rest correctly
	let virt_addr_stack_start = virt_addr_start.add(BasePageSize::SIZE);
	let phys_addr_stack_start = phys_addr_start.add(BasePageSize::SIZE);

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().execute_disable();
	paging::map::<BasePageSize>(
		virt_addr_stack_start,
		phys_addr_stack_start,
		num_pages,
		flags,
	);

	// Clear the stack
	unsafe {
		virt_addr_stack_start
			.as_mut_ptr::<u8>()
			.write_bytes(0, num_pages * BasePageSize::SIZE as usize);
	};

	// Insert marker on top
	let marker_addr = virt_addr_start + size - MARKER_SIZE;
	unsafe {
		marker_addr.as_mut_ptr::<u64>().write(MARKER);
	}

	StackAllocation {
		virt_addr: virt_addr_start,
		phys_addr: phys_addr_start,
		stack_size: size,
		weak: false,
	}
}

pub struct StackAllocation {
	/// Start address of allocated virtual memory region
	virt_addr: VirtAddr,
	/// Start address of allocated virtual memory region
	phys_addr: PhysAddr,
	/// Number of pages of this stack, including guard page
	stack_size: usize,

	/// If true, this is a weak reference to a stack that should not be freed
	weak: bool,
}

impl Drop for StackAllocation {
	fn drop(&mut self) {
		if self.weak || self.phys_addr.is_null() {
			return;
		}

		assert!(self.stack_size.is_multiple_of(BasePageSize::SIZE as usize));

		paging::unmap::<BasePageSize>(
			self.virt_addr,
			self.stack_size / BasePageSize::SIZE as usize,
		);

		// Release memory
		let virt_range =
			PageRange::from_start_len(self.virt_addr.as_usize(), self.stack_size).unwrap();
		unsafe {
			STACK_FREE_LIST
				.lock()
				.deallocate(virt_range)
				.expect("failed to free stack memory");
		}

		let phys_range =
			PageRange::from_start_len(self.phys_addr.as_usize(), self.stack_size).unwrap();
		unsafe {
			FrameAlloc::deallocate(phys_range);
		}
	}
}

impl StackAllocation {
	/// Returns the available stack size in this stack (end to start, without marker or guard)
	pub fn stack_size(&self) -> usize {
		self.stack_size - BasePageSize::SIZE as usize - MARKER_SIZE
	}

	/// Returns the last address in the stack
	pub fn stack_end(&self) -> VirtAddr {
		self.virt_addr.add(self.stack_size)
	}

	/// Returns the top of the stack, that is the last address with the marker excluded
	pub fn top_of_stack(&self) -> VirtAddr {
		self.stack_end() - MARKER_SIZE
	}

	/// Returns the first address usable in the stack
	pub fn stack_start(&self) -> VirtAddr {
		self.virt_addr.add(BasePageSize::SIZE)
	}

	/// Returns the stack guard page (excluded from [Self::stack_end] / [Self::stack_start] / [Self::stack_size])
	pub fn stack_guard_start(&self) -> VirtAddr {
		self.virt_addr
	}

	/// Returns a clone of this stack allocation that will not cause memory to be leaked when
	/// dropped
	pub fn weak(&self) -> StackAllocation {
		StackAllocation {
			weak: true,
			stack_size: self.stack_size,
			phys_addr: self.phys_addr,
			virt_addr: self.virt_addr,
		}
	}

	pub fn leak(mut self) -> Self {
		self.weak = true;
		self
	}

	/// Create a stack alloc for an externally created stack.
	/// No stack protection will be available for that stack.
	///
	/// # Safety
	///
	/// The address must be mapped and correspond to a stack of the provided size
	#[cfg(target_arch = "aarch64")]
	pub unsafe fn new_external(top_of_stack: VirtAddr, size: usize) -> Self {
		let phys = PhysAddr::zero();

		Self {
			virt_addr: top_of_stack - size,
			phys_addr: phys,
			stack_size: size,
			weak: true,
		}
	}

	#[cfg(target_arch = "riscv64")]
	pub unsafe fn new_bootstack(size: usize) -> Self {
		Self {
			virt_addr: VirtAddr::zero(),
			phys_addr: PhysAddr::zero(),
			stack_size: size,
			weak: true,
		}
	}
}

impl Display for StackAllocation {
	fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
		write!(
			f,
			"Stack Allocation: usable range {:x}..{:x}, guard page: {:x?}",
			self.stack_start().as_usize(),
			self.top_of_stack().as_usize(),
			self.stack_guard_start()
		)
	}
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
pub fn is_in_stack_range(address: x86_64::addr::VirtAddr) -> bool {
	let address_usize = address.as_u64() as usize;
	address_usize < STACK_REGION_END && address_usize >= STACK_REGION_START
}
