use core::alloc::AllocError;

use free_list::PageLayout;
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::mm::{FrameBox, PageBox};

/// A range of pages that is mapped for as long as this box is alive.
pub struct MappedPageBox {
	_frames: Option<FrameBox>,
	pages: PageBox,
}

impl MappedPageBox {
	/// Allocates the pages and frames described by `layout` and maps them.
	pub fn new(layout: PageLayout, flags: PageTableEntryFlags) -> Result<Self, AllocError> {
		let frames = FrameBox::new(layout)?;
		let pages = PageBox::new(layout)?;
		let page_count = pages.len().get() / BasePageSize::SIZE as usize;
		paging::map::<BasePageSize>(
			VirtAddr::from(pages.start()),
			PhysAddr::from(frames.start()),
			page_count,
			flags,
		);

		Ok(Self {
			_frames: Some(frames),
			pages,
		})
	}

	/// Allocates the pages described by `layout` and maps them to `phys_addr`.
	///
	/// # Safety
	///
	/// - The frames at `phys_addr` must not be deallocated while the returned box is alive.
	pub unsafe fn map_phys(
		phys_addr: PhysAddr,
		layout: PageLayout,
		flags: PageTableEntryFlags,
	) -> Result<Self, AllocError> {
		let pages = PageBox::new(layout)?;
		let page_count = pages.len().get() / BasePageSize::SIZE as usize;
		paging::map::<BasePageSize>(VirtAddr::from(pages.start()), phys_addr, page_count, flags);

		Ok(Self {
			_frames: None,
			pages,
		})
	}

	pub fn pages(&self) -> &PageBox {
		&self.pages
	}
}

impl Drop for MappedPageBox {
	fn drop(&mut self) {
		let page_count = self.pages.len().get() / BasePageSize::SIZE as usize;
		paging::unmap::<BasePageSize>(VirtAddr::from(self.pages.start()), page_count);
	}
}
