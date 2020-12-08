// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![allow(dead_code)]

use crate::arch::x86_64::kernel::gdt;
use crate::x86::bits64::paging::VAddr;
use crate::x86::dtables::{self, DescriptorTablePointer};
use crate::x86::segmentation::{SegmentSelector, SystemDescriptorTypes64};
use crate::x86::Ring;
use core::sync::atomic::{AtomicBool, Ordering};

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

enum Type {
	InterruptGate,
	TrapGate,
}

impl Type {
	pub fn pack(self) -> u8 {
		match self {
			Type::InterruptGate => SystemDescriptorTypes64::InterruptGate as u8,
			Type::TrapGate => SystemDescriptorTypes64::TrapGate as u8,
		}
	}
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
		flags: 0,
		base_hi: 0,
		reserved1: 0,
	};

	/// Create a new IdtEntry pointing at `handler`, which must be a function
	/// with interrupt calling conventions.  (This must be currently defined in
	/// assembly language.)  The `gdt_code_selector` value must be the offset of
	/// code segment entry in the GDT.
	///
	/// The "Present" flag set, which is the most common case.  If you need
	/// something else, you can construct it manually.
	pub fn new(
		handler: VAddr,
		gdt_code_selector: SegmentSelector,
		dpl: Ring,
		ty: Type,
		ist_index: u8,
	) -> IdtEntry {
		assert!(ist_index < 0b1000);
		IdtEntry {
			base_lo: ((handler.as_usize() as u64) & 0xFFFF) as u16,
			base_hi: handler.as_usize() as u64 >> 16,
			selector: gdt_code_selector,
			ist_index,
			flags: dpl as u8 | ty.pack() | (1 << 7),
			reserved1: 0,
		}
	}
}

/// Declare an IDT of 256 entries.
/// Although not all entries are used, the rest exists as a bit
/// of a trap. If any undefined IDT entry is hit, it will cause
/// an "Unhandled Interrupt" exception.
pub const IDT_ENTRIES: usize = 256;

#[repr(align(4096))]
struct IdtArray {
	entries: [IdtEntry; IDT_ENTRIES],
}

impl IdtArray {
	pub const fn new() -> Self {
		IdtArray {
			entries: [IdtEntry::MISSING; IDT_ENTRIES],
		}
	}
}

static mut IDT: IdtArray = IdtArray::new();
static mut IDTP: DescriptorTablePointer<IdtEntry> = DescriptorTablePointer {
	base: 0 as *const IdtEntry,
	limit: 0,
};

pub fn install() {
	static IDT_INIT: AtomicBool = AtomicBool::new(false);

	unsafe {
		let is_init = IDT_INIT.swap(true, Ordering::SeqCst);

		if !is_init {
			debug!("IDT address: 0x{:x}", &IDT.entries as *const _ as usize);

			// TODO: As soon as https://github.com/rust-lang/rust/issues/44580 is implemented, it should be possible to
			// implement "new" as "const fn" and do this call already in the initialization of IDTP.
			IDTP = DescriptorTablePointer::new_from_slice(&IDT.entries);
		};

		dtables::lidt(&IDTP);
	}
}

/// Set an entry in the IDT.
///
/// # Arguments
///
/// * `index`     - 8-bit index of the interrupt gate to set.
/// * `handler`   - Handler function to call for this interrupt/exception.
/// * `ist_index` - Index of the Interrupt Stack Table (IST) to switch to.
///                 A zero value means that the stack won't be switched, a value of 1 refers to the first IST entry, etc.
pub fn set_gate(index: u8, handler: usize, ist_index: u8) {
	let sel = SegmentSelector::new(gdt::GDT_KERNEL_CODE, Ring::Ring0);
	let entry = IdtEntry::new(
		VAddr::from_usize(handler),
		sel,
		Ring::Ring0,
		Type::InterruptGate,
		ist_index,
	);

	unsafe {
		IDT.entries[index as usize] = entry;
	}
}
