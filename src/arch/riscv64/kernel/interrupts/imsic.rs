//! Message Signaled Interrupt Controller (IMSIC) driver for RISC-V.
//!
//! [RISC-V Advanced Interrupt Architecture]: https://github.com/riscv/riscv-aia/releases/download/20250312/riscv-interrupts-20250312.pdf

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::num::NonZeroU16;
use core::ptr::NonNull;

use align_address::Align;
use free_list::PageLayout;
use memory_addresses::{PhysAddr, VirtAddr};
use riscv::register::{sireg, siselect, stopei};
use volatile::access::{NoAccess, WriteOnly};
use volatile::{VolatileFieldAccess, VolatileRef};

use crate::arch::kernel::interrupts::MSI_EIID_WAKEUP;
use crate::arch::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::arch::riscv64::kernel::core_local::set_msi_controller;
use crate::init_cell::InitCell;
use crate::mm::{PageAlloc, PageRangeAllocator};

// Address of interrupt files for each hart index by hart_id.
// Use HARTS_AVAILABLE to map CpuId to hart_id.
pub(crate) static INTERRUPT_FILES: InitCell<Vec<VirtAddr>> = InitCell::new(Vec::new());

#[repr(C)]
#[derive(VolatileFieldAccess)]
pub(crate) struct InterruptFile {
	#[access(WriteOnly)]
	seteipnum_le: u32,

	#[access(WriteOnly)]
	_seteipnum_be: u32,

	#[access(NoAccess)]
	__: [u32; 0x3fe],
}
const _: () = assert!(size_of::<InterruptFile>() == 0x1000);

#[repr(usize)]
enum Eidelivery {
	// Interrupt delivery disabled
	_Disabled = 0,

	// Interrupt delivery from the interrupt file is enabled
	ViaInterruptFile = 1,

	// Interrupt delivery from a PLIC or APLIC is enabled.
	// Support is option.
	_ViaExternalController = 0x4000_0000,
}

#[repr(usize)]
enum ISelect {
	// Interrupt delivery mode.
	Eidelivery = 0x70,

	// Interrupt priority threshold
	Eithreshold = 0x72,

	// Interrupt ending bits
	_Eip0 = 0x80,
	_Eip63 = 0xbf,

	// Interrupt enable bits
	Eie0 = 0xc0,
	Eie63 = 0xff,
}

pub(crate) struct Imsic {
	max_vectors: u16,
}

// IRQ numbers are reused as EIID (external interrupt identifier aka msi vector).
// This greatly simplifies support for IMSIC:
// - No per core msi pool allocator
// - No per core mapping from EIID to IRQ number
// - No per core handler map
// This work if the following assumptions are true:
// - The range of EIIDs supported by the IMSICs is a superset of the range of IRQ numbers
//   supported by the APLIC.
// This adds the following limitations:
// - IPIs might be more expensive if they collide with other IRQs.
impl Imsic {
	fn new(max_vectors: u16) -> Self {
		Self { max_vectors }
	}

	fn read(&mut self, index: usize) -> usize {
		assert!(index & 1 == 0, "If XLEN=64, the index must be even");
		unsafe {
			siselect::write(siselect::Siselect::from_bits(index));
		}
		sireg::read().bits()
	}

	fn write(&mut self, index: usize, value: usize) {
		unsafe {
			siselect::write(siselect::Siselect::from_bits(index));
			sireg::write(sireg::Sireg::from_bits(value));
		}
	}

	fn set_interrupt_delivery_mode(&mut self, mode: Eidelivery) {
		self.write(ISelect::Eidelivery as usize, mode as usize);
	}

	pub fn set_interrupt_priority_threshold(&mut self, threshold: u8) {
		assert!(
			threshold == 0 || threshold >= MSI_EIID_WAKEUP as u8,
			"IPIs shall not be masked by the priority threshold"
		);
		self.write(ISelect::Eithreshold as usize, threshold as usize);
	}

	pub fn set_interrupt_enable(&mut self, eiid: NonZeroU16, value: bool) {
		assert!(eiid.get() < self.max_vectors);

		let eiid = eiid.get() as usize;
		let eie_index = ISelect::Eie0 as usize + ((eiid / 64) * 2);
		assert!(
			eie_index <= ISelect::Eie63 as usize,
			"Interrupt number {eiid} is out of range for Imsic"
		);
		let bit_position = eiid % 64;
		let current_value = self.read(eie_index);
		if value {
			self.write(eie_index, current_value | (1 << bit_position));
		} else {
			self.write(eie_index, current_value & !(1 << bit_position));
		}
	}

	pub fn claim_interrupt(&mut self) -> Option<NonZeroU16> {
		unsafe { stopei::read_clear() }
			.iid()
			.try_into()
			.ok()
			.and_then(NonZeroU16::new)
	}

	pub fn complete_interrupt(&mut self, _eiid: NonZeroU16) {
		// atomic read and write of stopic register automatically completes the interrupt
	}

	pub fn set_ipi(&mut self, hart_id: usize, eiid: NonZeroU16) {
		assert!(eiid.get() < self.max_vectors);
		let interrupt_file_addr = INTERRUPT_FILES.get().unwrap()[hart_id];
		let mut interrupt_file =
			unsafe { VolatileRef::new(NonNull::new(interrupt_file_addr.as_mut_ptr()).unwrap()) };
		interrupt_file
			.as_mut_ptr()
			.seteipnum_le()
			.write(u32::from(eiid.get()));
	}
}

pub fn init_interrupt_files(addr: PhysAddr, size: usize, interrupt_file_indices: Vec<usize>) {
	assert!(
		addr.is_aligned_to(BasePageSize::SIZE),
		"Imsic control region is not page aligned"
	);
	assert!(
		size.is_multiple_of(usize::try_from(BasePageSize::SIZE).unwrap()),
		"Imsic control region size is not a multiple of a page"
	);

	let layout = PageLayout::from_size(size).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let interrupt_file_base_addr = VirtAddr::from(page_range.start());

	let mut flags = PageTableEntryFlags::empty();
	flags.device().normal().writable().execute_disable();
	paging::map::<BasePageSize>(
		interrupt_file_base_addr,
		addr,
		size / usize::try_from(BasePageSize::SIZE).unwrap(),
		flags,
	);

	INTERRUPT_FILES.with(|files| {
		*files.unwrap() = interrupt_file_indices
			.into_iter()
			.map(|index| {
				let hart_addr = interrupt_file_base_addr + (index * size_of::<InterruptFile>());
				assert!(hart_addr < interrupt_file_base_addr + size);
				hart_addr
			})
			.collect();
	});
	INTERRUPT_FILES.finalize();
}

pub(crate) fn init_imsic(max_vectors: u16) {
	let mut imsic = Box::new(Imsic::new(max_vectors));

	imsic.set_interrupt_delivery_mode(Eidelivery::ViaInterruptFile);

	// Enable MSI used for IPI
	#[cfg(feature = "smp")]
	imsic.set_interrupt_enable(NonZeroU16::new(MSI_EIID_WAKEUP).unwrap(), true);

	set_msi_controller(Box::into_raw(imsic));
}
