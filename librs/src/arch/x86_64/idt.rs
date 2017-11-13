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

use arch::x86_64::gdt;
use spin;
use x86::bits64::irq::{IdtEntry, Type};
use x86::shared::dtables::{self, DescriptorTablePointer};
use x86::shared::paging::VAddr;
use x86::shared::PrivilegeLevel;
use x86::shared::segmentation::SegmentSelector;

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
			IDTP = DescriptorTablePointer::new(&IDT);
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
#[inline]
pub fn set_gate(index: u8, handler: unsafe extern "C" fn(), ist_index: u8)
{
	let sel = SegmentSelector::new(gdt::GDT_KERNEL_CODE, PrivilegeLevel::Ring0);
	let entry = IdtEntry::new(VAddr::from_usize(handler as usize), sel, PrivilegeLevel::Ring0, Type::InterruptGate, ist_index);

	unsafe { IDT[index as usize] = entry; }
}

#[no_mangle]
pub unsafe extern "C" fn idt_install() {
	install();
}
