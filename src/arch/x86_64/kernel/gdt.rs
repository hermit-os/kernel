// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::x86_64::kernel::percore::*;
use crate::arch::x86_64::kernel::BOOT_INFO;
use crate::config::*;
use crate::x86::bits64::segmentation::*;
use crate::x86::bits64::task::*;
use crate::x86::dtables::{self, DescriptorTablePointer};
use crate::x86::segmentation::*;
use crate::x86::task::*;
use crate::x86::Ring;
use alloc::boxed::Box;
use core::mem;

pub const GDT_NULL: u16 = 0;
pub const GDT_KERNEL_CODE: u16 = 1;
pub const GDT_KERNEL_DATA: u16 = 2;
pub const GDT_FIRST_TSS: u16 = 3;

/// We dynamically allocate a GDT large enough to hold the maximum number of entries.
const GDT_ENTRIES: usize = 8192;

/// We use IST1 through IST4.
/// Each critical exception (NMI, Double Fault, Machine Check) gets a dedicated one while IST1 is shared for all other
/// interrupts. See also irq.rs.
const IST_ENTRIES: usize = 4;

static mut GDT: Gdt = Gdt::new();
static mut GDTR: DescriptorTablePointer<Descriptor> = DescriptorTablePointer {
	base: 0 as *const Descriptor,
	limit: 0,
};

#[repr(align(4096))]
struct Gdt {
	entries: [Descriptor; GDT_ENTRIES],
}

impl Gdt {
	pub const fn new() -> Self {
		Gdt {
			entries: [Descriptor::NULL; GDT_ENTRIES],
		}
	}
}
pub fn init() {
	unsafe {
		// The NULL descriptor is always the first entry.
		GDT.entries[GDT_NULL as usize] = Descriptor::NULL;

		// The second entry is a 64-bit Code Segment in kernel-space (Ring 0).
		// All other parameters are ignored.
		GDT.entries[GDT_KERNEL_CODE as usize] =
			DescriptorBuilder::code_descriptor(0, 0, CodeSegmentType::ExecuteRead)
				.present()
				.dpl(Ring::Ring0)
				.l()
				.finish();

		// The third entry is a 64-bit Data Segment in kernel-space (Ring 0).
		// All other parameters are ignored.
		GDT.entries[GDT_KERNEL_DATA as usize] =
			DescriptorBuilder::data_descriptor(0, 0, DataSegmentType::ReadWrite)
				.present()
				.dpl(Ring::Ring0)
				.finish();

		// Let GDTR point to our newly crafted GDT.
		GDTR = DescriptorTablePointer::new_from_slice(&(GDT.entries[0..GDT_ENTRIES]));
	}
}

pub fn add_current_core() {
	unsafe {
		// Load the GDT for the current core.
		dtables::lgdt(&GDTR);

		// Reload the segment descriptors
		load_cs(SegmentSelector::new(GDT_KERNEL_CODE, Ring::Ring0));
		load_ds(SegmentSelector::new(GDT_KERNEL_DATA, Ring::Ring0));
		load_es(SegmentSelector::new(GDT_KERNEL_DATA, Ring::Ring0));
		load_ss(SegmentSelector::new(GDT_KERNEL_DATA, Ring::Ring0));
	}

	// Dynamically allocate memory for a Task-State Segment (TSS) for this core.
	let mut boxed_tss = Box::new(TaskStateSegment::new());

	// Every task later gets its own stack, so this boot stack is only used by the Idle task on each core.
	// When switching to another task on this core, this entry is replaced.
	boxed_tss.rsp[0] = unsafe { core::ptr::read_volatile(&(*BOOT_INFO).current_stack_address) }
		+ KERNEL_STACK_SIZE as u64
		- 0x10u64;
	set_kernel_stack(boxed_tss.rsp[0] as u64);

	// Allocate all ISTs for this core.
	// Every task later gets its own IST1, so the IST1 allocated here is only used by the Idle task.
	for i in 0..IST_ENTRIES {
		let ist = crate::mm::allocate(KERNEL_STACK_SIZE, true);
		boxed_tss.ist[i] = ist.as_u64() + KERNEL_STACK_SIZE as u64 - 0x10u64;
	}

	unsafe {
		// Add this TSS to the GDT.
		let idx = GDT_FIRST_TSS as usize + (core_id() as usize) * 2;
		let tss = Box::into_raw(boxed_tss);
		{
			let base = tss as u64;
			let tss_descriptor: Descriptor64 =
				<DescriptorBuilder as GateDescriptorBuilder<u64>>::tss_descriptor(
					base,
					base + mem::size_of::<TaskStateSegment>() as u64 - 1,
					true,
				)
				.present()
				.dpl(Ring::Ring0)
				.finish();
			GDT.entries[idx..idx + 2]
				.copy_from_slice(&mem::transmute::<Descriptor64, [Descriptor; 2]>(
					tss_descriptor,
				));
		}

		// Load it.
		let sel = SegmentSelector::new(idx as u16, Ring::Ring0);
		load_tr(sel);

		// Store it in the PerCoreVariables structure for further manipulation.
		PERCORE.tss.set(tss);
	}
}

#[inline(never)]
#[no_mangle]
pub fn set_current_kernel_stack() {
	core_scheduler().set_current_kernel_stack();
}
