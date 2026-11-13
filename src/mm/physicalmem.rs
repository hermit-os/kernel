use core::alloc::AllocError;
use core::{fmt, iter, ptr, slice};

use align_address::Align;
use free_list::{PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
#[cfg(feature = "hermit-entry")]
use memory_addresses::PhysAddr;
use memory_addresses::VirtAddr;

#[cfg(all(target_arch = "x86_64", feature = "hermit-entry"))]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{self, BasePageSize, HugePageSize, LargePageSize, PageSize};
use crate::env::{self, MemmapType, StartInfo};
use crate::mm::bitmap::FrameBitmap;
use crate::mm::device_alloc::DeviceAlloc;
use crate::mm::{PageRangeAllocator, PageRangeBox};

/// Size of a physical frame, the allocation granularity of [`FrameAlloc`].
pub(crate) const FRAME_SIZE: usize = BasePageSize::SIZE as usize;

static PHYSICAL_FRAMES: InterruptTicketMutex<FrameBitmap<'static>> =
	InterruptTicketMutex::new(FrameBitmap::empty());

pub struct FrameAlloc;

impl PageRangeAllocator for FrameAlloc {
	unsafe fn init() -> usize {
		if cfg!(target_arch = "x86_64") && DeviceAlloc.phys_offset() != VirtAddr::zero() {
			let start = DeviceAlloc.phys_offset();
			let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
			let count = usize::try_from(count).unwrap();
			paging::unmap::<HugePageSize>(start, count);
		}

		unsafe { detect_from_start_info() }
	}

	fn allocate(layout: PageLayout) -> Result<PageRange, AllocError> {
		PHYSICAL_FRAMES.lock().allocate(layout)
	}

	fn allocate_at(range: PageRange) -> Result<(), AllocError> {
		PHYSICAL_FRAMES.lock().allocate_at(range)
	}

	unsafe fn deallocate(range: PageRange) {
		PHYSICAL_FRAMES.lock().deallocate(range);
	}
}

impl fmt::Display for FrameAlloc {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let bitmap = PHYSICAL_FRAMES.lock();
		write!(f, "FrameAlloc free ranges:")?;
		for range in bitmap.free_ranges() {
			write!(f, "\n{range:#}")?;
		}
		Ok(())
	}
}

pub type FrameBox = PageRangeBox<FrameAlloc>;

#[cfg(feature = "hermit-entry")]
pub unsafe fn map_frame_range(frame_range: PageRange) {
	use crate::arch::mm::paging::PageTableEntryFlags;

	cfg_select! {
		target_arch = "aarch64" => {
			type IdentityPageSize = BasePageSize;
		}
		target_arch = "riscv64" => {
			type IdentityPageSize = HugePageSize;
		}
		target_arch = "x86_64" => {
			type IdentityPageSize = LargePageSize;
		}
	}

	let start = frame_range
		.start()
		.align_down(IdentityPageSize::SIZE.try_into().unwrap());
	let end = frame_range
		.end()
		.align_up(IdentityPageSize::SIZE.try_into().unwrap());

	(start..end)
		.step_by(IdentityPageSize::SIZE.try_into().unwrap())
		.map(|addr| PhysAddr::new(addr.try_into().unwrap()))
		.for_each(paging::identity_map::<IdentityPageSize>);

	// Map the physical memory again if DeviceAlloc operates at an offset
	if DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let flags = PageTableEntryFlags::normal();
		(start..end)
			.step_by(IdentityPageSize::SIZE.try_into().unwrap())
			.for_each(|addr| {
				let phys_addr = PhysAddr::new(addr.try_into().unwrap());
				let virt_addr = VirtAddr::from_ptr(DeviceAlloc.ptr_from::<()>(phys_addr));
				paging::map::<IdentityPageSize>(virt_addr, phys_addr, 1, flags);
			});
	}
}

/// Returns the RAM ranges from the memory map that the kernel may claim.
fn usable_ram() -> impl Iterator<Item = PageRange> {
	env::start_info()
		.memmap()
		.filter(|memmap_entry| memmap_entry.ty == MemmapType::Ram)
		.filter_map(|memmap_entry| {
			let mut start_addr = memmap_entry.phys_addr;
			let mut end_addr = start_addr + memmap_entry.len;

			// Do not free any address in the real mode addressable range.
			//
			// When claiming physical memory, ignore all addresses below this one. This ensures we
			// don't accidentally clash with hardcoded low addresses, such as `SMP_BOOT_CODE_ADDRESS`
			// in x86_64 with SMP enabled. We use a 2MIB size for now, but this is arbitrary, and could
			// likely be lowered.
			start_addr = start_addr.max(LargePageSize::SIZE as usize);

			#[cfg(all(target_arch = "x86_64", feature = "hermit-entry"))]
			if paging::is_recursive() {
				start_addr = start_addr.max(elf_symbols::executable_end().addr());
			}

			if cfg!(target_arch = "aarch64") || cfg!(target_arch = "riscv64") {
				start_addr = start_addr.max(elf_symbols::executable_end().addr());
			}

			start_addr = start_addr.align_up(0x1000);
			end_addr = end_addr.align_down(0x1000);

			PageRange::new(start_addr, end_addr).ok()
		})
}

fn reservations() -> impl Iterator<Item = PageRange> {
	let kernel_start = elf_symbols::executable_start().addr();
	let kernel_end = elf_symbols::executable_end().addr();
	let kernel_region = PageRange::containing(kernel_start, kernel_end).unwrap();

	let modules = env::start_info()
		.modules()
		.map(|module| module.phys_frame_range());

	#[cfg(feature = "hermit-entry")]
	let fdt = {
		use crate::env::FdtStartInfo;

		let fdt = env::start_info().fdt().unwrap();

		let fdt_start = env::start_info().fdt_addr().unwrap().get();
		let fdt_end = fdt_start + fdt.total_size();
		let fdt_region = PageRange::containing(fdt_start, fdt_end).unwrap();

		fdt.memory_reservations()
			.map(|reservation| {
				let start = reservation.address().addr();
				let end = start + reservation.size();
				PageRange::new(start, end).unwrap()
			})
			.chain(iter::once(fdt_region))
	};
	#[cfg(not(feature = "hermit-entry"))]
	let fdt = iter::empty();

	iter::once(kernel_region).chain(modules).chain(fdt)
}

/// Returns [`usable_ram`] without [`reservations`].
fn free_ranges() -> impl Iterator<Item = PageRange> {
	usable_ram().flat_map(|ram| {
		let mut start = ram.start();
		iter::from_fn(move || {
			while let Ok(rest) = PageRange::new(start, ram.end()) {
				let Some(reservation) = reservations()
					.filter_map(|reservation| reservation.and(rest))
					.min_by_key(|reservation| reservation.start())
				else {
					start = ram.end();
					return Some(rest);
				};
				let free = PageRange::new(start, reservation.start());
				start = reservation.end();
				if let Ok(free) = free {
					return Some(free);
				}
			}
			None
		})
	})
}

/// returns the available memory (excluding the bitmap) in Bytes.
unsafe fn detect_from_start_info() -> usize {
	for reservation in reservations() {
		debug!("Memory reservation: {reservation:#x?}");
	}

	let start = free_ranges().map(PageRange::start).min().unwrap();
	let end = free_ranges().map(PageRange::end).max().unwrap();
	let managed = PageRange::new(start, end).unwrap();
	let bitmap_words = FrameBitmap::required_words(managed);
	let bitmap_size = (bitmap_words * size_of::<u64>()).align_up(FRAME_SIZE);

	// The bitmap is placed in the lowest free memory.
	// We assume this memory is already mapped, or that no new pagetables are
	// required during the setup.
	let bitmap_range = free_ranges()
		.filter_map(|range| {
			PageRange::from_start_len(range.start(), bitmap_size)
				.ok()
				.filter(|fit| range.contains(*fit))
		})
		.min_by_key(|fit| fit.start())
		.unwrap();
	debug!("Frame bitmap: {bitmap_range:#x?}");

	// We have to assume working identity mapping here, as the `DeviceAlloc` mapping is
	// only established by `map_frame_range` below, which requires the bitmap.
	let storage = unsafe {
		let ptr = ptr::with_exposed_provenance_mut::<u64>(bitmap_range.start());
		slice::from_raw_parts_mut(ptr, bitmap_words)
	};
	let mut bitmap = FrameBitmap::new(managed, storage);
	for frame_range in free_ranges() {
		bitmap.deallocate(frame_range);
	}
	bitmap.allocate_at(bitmap_range).unwrap();
	*PHYSICAL_FRAMES.lock() = bitmap;

	for frame_range in free_ranges() {
		#[cfg(feature = "hermit-entry")]
		unsafe {
			map_frame_range(frame_range);
		}
		debug!("Claimed physical memory: {frame_range:#x?}");
	}

	let free_frames = PHYSICAL_FRAMES.lock().free_frames();
	free_frames * FRAME_SIZE
}
