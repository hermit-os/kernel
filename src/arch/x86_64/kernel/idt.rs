// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

#![allow(dead_code)]

use arch::x86_64::kernel::gdt;
use spin;
use x86::Ring;
use x86::bits64::paging::VAddr;
use x86::dtables::{self, DescriptorTablePointer};
use x86::segmentation::{SegmentSelector,SystemDescriptorTypes64};

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
    TrapGate
}

impl Type {
    pub fn pack(self) -> u8 {
        match self {
            Type::InterruptGate => SystemDescriptorTypes64::InterruptGate as u8,
			Type::TrapGate => SystemDescriptorTypes64::TrapGate as u8
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
    pub fn new(handler: VAddr, gdt_code_selector: SegmentSelector,
               dpl: Ring, ty: Type, ist_index: u8) -> IdtEntry {
        assert!(ist_index < 0b1000);
        IdtEntry {
            base_lo: ((handler.as_usize() as u64) & 0xFFFF) as u16,
            base_hi: handler.as_usize() as u64 >> 16,
            selector: gdt_code_selector,
            ist_index: ist_index,
            flags: dpl as u8
                |  ty.pack()
                |  (1 << 7),
            reserved1: 0,
        }
    }
}

/// Declare an IDT of 256 entries.
/// Although not all entries are used, the rest exists as a bit
/// of a trap. If any undefined IDT entry is hit, it will cause
/// an "Unhandled Interrupt" exception.
pub const IDT_ENTRIES: usize = 256;

static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry::MISSING; IDT_ENTRIES];
static mut IDTP: DescriptorTablePointer<IdtEntry> = DescriptorTablePointer { base: 0 as *const IdtEntry, limit: 0 };
static IDT_INIT: spin::Once<()> = spin::Once::new();

pub fn install() {
	unsafe {
		IDT_INIT.call_once(|| {
			// TODO: As soon as https://github.com/rust-lang/rust/issues/44580 is implemented, it should be possible to
			// implement "new" as "const fn" and do this call already in the initialization of IDTP.
			IDTP = DescriptorTablePointer::new_from_slice(&IDT);
		});

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
pub fn set_gate(index: u8, handler: usize, ist_index: u8)
{
	let sel = SegmentSelector::new(gdt::GDT_KERNEL_CODE, Ring::Ring0);
	let entry = IdtEntry::new(VAddr::from_usize(handler), sel, Ring::Ring0, Type::InterruptGate, ist_index);

	unsafe { IDT[index as usize] = entry; }
}
