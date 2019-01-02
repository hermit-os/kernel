// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2017 Colin Finck, RWTH Aachen University
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

include!(concat!(env!("CARGO_TARGET_DIR"), "/config.rs"));

use alloc::boxed::Box;
use arch::x86_64::kernel::percore::*;
use core::mem;
use scheduler::task::TaskStatus;
use x86::bits64::segmentation::*;
use x86::bits64::task::*;
use x86::shared::PrivilegeLevel;
use x86::shared::dtables::{self, DescriptorTablePointer};
use x86::shared::task::*;


extern "C" {
	static current_stack_address: usize;
}


pub const GDT_KERNEL_CODE: u16 = 1;
pub const GDT_KERNEL_DATA: u16 = 2;
pub const GDT_FIRST_TSS:   u16 = 3;

/// We dynamically allocate a GDT large enough to hold the maximum number of entries.
const GDT_ENTRIES: usize = 8192;

/// We use IST1 through IST4.
/// Each critical exception (NMI, Double Fault, Machine Check) gets a dedicated one while IST1 is shared for all other
/// interrupts. See also irq.rs.
const IST_ENTRIES: usize = 4;

static mut GDT: *mut Gdt = 0 as *mut Gdt;
static mut GDTR: DescriptorTablePointer<SegmentDescriptor> = DescriptorTablePointer { base: 0 as *const SegmentDescriptor, limit: 0 };

struct Gdt {
	entries: [SegmentDescriptor; GDT_ENTRIES]
}


pub fn init() {
	unsafe {
		// Dynamically allocate memory for the GDT.
		GDT = ::mm::allocate(mem::size_of::<Gdt>(), true) as *mut Gdt;

		// The NULL descriptor is always the first entry.
		(*GDT).entries[0] = SegmentDescriptor::NULL;

		// The second entry is a 64-bit Code Segment in kernel-space (Ring 0).
		// All other parameters are ignored.
		(*GDT).entries[GDT_KERNEL_CODE as usize] = SegmentDescriptor::new_memory(0, 0, Type::Code(CODE_READ), false, PrivilegeLevel::Ring0, SegmentBitness::Bits64);

		// The third entry is a 64-bit Data Segment in kernel-space (Ring 0).
		// All other parameters are ignored.
		(*GDT).entries[GDT_KERNEL_DATA as usize] = SegmentDescriptor::new_memory(0, 0, Type::Data(DATA_WRITE), false, PrivilegeLevel::Ring0, SegmentBitness::Bits64);

		// Let GDTR point to our newly crafted GDT.
		GDTR = DescriptorTablePointer::new(&((*GDT).entries));
	}
}

pub fn add_current_core() {
	unsafe {
		// Load the GDT for the current core.
		dtables::lgdt(&GDTR);

		// Reload the segment descriptors
		set_cs(SegmentSelector::new(GDT_KERNEL_CODE as u16, PrivilegeLevel::Ring0));
		load_ds(SegmentSelector::new(GDT_KERNEL_DATA as u16, PrivilegeLevel::Ring0));
		load_es(SegmentSelector::new(GDT_KERNEL_DATA as u16, PrivilegeLevel::Ring0));
		load_ss(SegmentSelector::new(GDT_KERNEL_DATA as u16, PrivilegeLevel::Ring0));
	}

	// Dynamically allocate memory for a Task-State Segment (TSS) for this core.
	let mut boxed_tss = Box::new(TaskStateSegment::new());

	// Every task later gets its own stack, so this boot stack is only used by the Idle task on each core.
	// When switching to another task on this core, this entry is replaced.
	boxed_tss.rsp[0] = (unsafe { current_stack_address } + KERNEL_STACK_SIZE - 0x10) as u64;

	// Allocate all ISTs for this core.
	// Every task later gets its own IST1, so the IST1 allocated here is only used by the Idle task.
	for i in 0..IST_ENTRIES {
		let ist = ::mm::allocate(KERNEL_STACK_SIZE, true);
		boxed_tss.ist[i] = (ist + KERNEL_STACK_SIZE - 0x10) as u64;
	}

	unsafe {
		// Add this TSS to the GDT.
		let idx = GDT_FIRST_TSS as usize + (core_id() as usize)*2;
		(*GDT).entries[idx..idx+2].copy_from_slice(&SegmentDescriptor::new_tss(boxed_tss.as_ref(), PrivilegeLevel::Ring0));

		// Load it.
		let sel = SegmentSelector::new(idx as u16, PrivilegeLevel::Ring0);
		load_tr(sel);

		// Store it in the PerCoreVariables structure for further manipulation.
		let tss = Box::into_raw(boxed_tss);
		PERCORE.tss.set(tss);
	}
}

pub fn get_boot_stacks() -> (usize, usize) {
	let tss = unsafe { &(*PERCORE.tss.get()) };

	let stack = tss.rsp[0] as usize;
	let ist = tss.ist[0] as usize;
	(stack, ist)
}

#[no_mangle]
pub extern "C" fn set_current_kernel_stack() {
	let current_task_borrowed = core_scheduler().current_task.borrow();
	let stack_size = if current_task_borrowed.status == TaskStatus::TaskIdle {
		KERNEL_STACK_SIZE
	} else {
		DEFAULT_STACK_SIZE
	};

	let tss = unsafe { &mut (*PERCORE.tss.get()) };

	tss.rsp[0] = (current_task_borrowed.stacks.stack + stack_size - 0x10) as u64;
	tss.ist[0] = (current_task_borrowed.stacks.ist + KERNEL_STACK_SIZE - 0x10) as u64;
}
