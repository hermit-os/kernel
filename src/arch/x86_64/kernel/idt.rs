use core::sync::atomic::{AtomicBool, Ordering};

use x86::dtables::{self, DescriptorTablePointer};
use x86::segmentation::SegmentSelector;

/// An interrupt gate descriptor.
///
/// See Intel manual 3a for details, specifically section "6.14.1 64-Bit Mode
/// IDT" and "Figure 6-7. 64-Bit IDT Gate Descriptors".
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
	/// Lower 16 bits of ISR.
	pub base_lo: u16,
	/// Segment selector.
	pub selector: SegmentSelector,
	/// This must always be zero.
	pub ist_index: u8,
	/// Flags.
	pub flags: u8,
	/// The upper 48 bits of ISR (the last 16 bits must be zero).
	pub base_hi: u64,
	/// Must be zero.
	pub reserved1: u16,
}

impl IdtEntry {
	/// A "missing" IdtEntry.
	///
	/// If the CPU tries to invoke a missing interrupt, it will instead
	/// send a General Protection fault (13), with the interrupt number and
	/// some other data stored in the error code.
	pub const MISSING: IdtEntry = IdtEntry {
		base_lo: 0,
		selector: SegmentSelector::from_raw(0),
		ist_index: 0,
		// InterruptGate
		flags: 0b1110,
		base_hi: 0,
		reserved1: 0,
	};
}

/// Declare an IDT of 256 entries.
/// Although not all entries are used, the rest exists as a bit
/// of a trap. If any undefined IDT entry is hit, it will cause
/// an "Unhandled Interrupt" exception.
pub const IDT_ENTRIES: usize = 256;

#[repr(align(4096))]
#[repr(C)]
pub struct IdtArray {
	entries: [IdtEntry; IDT_ENTRIES],
}

impl IdtArray {
	pub const fn new() -> Self {
		IdtArray {
			entries: [IdtEntry::MISSING; IDT_ENTRIES],
		}
	}
}

pub static mut IDT: IdtArray = IdtArray::new();
static mut IDTP: DescriptorTablePointer<IdtEntry> = DescriptorTablePointer {
	base: 0 as *const IdtEntry,
	limit: 0,
};

pub fn install() {
	static IDT_INIT: AtomicBool = AtomicBool::new(false);

	unsafe {
		let is_init = IDT_INIT.swap(true, Ordering::SeqCst);

		if !is_init {
			debug!("IDT address: {:#x}", &IDT.entries as *const _ as usize);

			// TODO: As soon as https://github.com/rust-lang/rust/issues/44580 is implemented, it should be possible to
			// implement "new" as "const fn" and do this call already in the initialization of IDTP.
			IDTP = DescriptorTablePointer::new_from_slice(&IDT.entries);
		};

		dtables::lidt(&IDTP);
	}
}
