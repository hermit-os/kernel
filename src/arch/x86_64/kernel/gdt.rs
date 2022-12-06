use alloc::boxed::Box;
use core::sync::atomic::Ordering;

use x86::bits64::segmentation::*;
use x86::bits64::task::*;
use x86::segmentation::*;
use x86::task::*;
use x86::Ring;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};

use super::interrupts::{IST_ENTRIES, IST_SIZE};
use super::scheduler::TaskStacks;
use super::CURRENT_STACK_ADDRESS;
use crate::arch::x86_64::kernel::percore::*;
use crate::config::*;

pub const GDT_KERNEL_CODE: u16 = 1;
pub const GDT_KERNEL_DATA: u16 = 2;
pub const GDT_FIRST_TSS: u16 = 3;

pub fn add_current_core() {
	let gdt = Box::leak(Box::new(GlobalDescriptorTable::new()));
	gdt.add_entry(Descriptor::kernel_code_segment());
	gdt.add_entry(Descriptor::kernel_data_segment());

	// Dynamically allocate memory for a Task-State Segment (TSS) for this core.
	let mut boxed_tss = Box::new(TaskStateSegment::new());

	// Every task later gets its own stack, so this boot stack is only used by the Idle task on each core.
	// When switching to another task on this core, this entry is replaced.
	boxed_tss.rsp[0] = CURRENT_STACK_ADDRESS.load(Ordering::Relaxed) + KERNEL_STACK_SIZE as u64
		- TaskStacks::MARKER_SIZE as u64;
	set_kernel_stack(boxed_tss.rsp[0]);

	// Allocate all ISTs for this core.
	// Every task later gets its own IST1, so the IST1 allocated here is only used by the Idle task.
	for i in 0..IST_ENTRIES {
		let ist = crate::mm::allocate(IST_SIZE, true);
		boxed_tss.ist[i] = ist.as_u64() + IST_SIZE as u64 - TaskStacks::MARKER_SIZE as u64;
	}

	let tss = Box::into_raw(boxed_tss);
	unsafe {
		PERCORE.tss.set(tss);
	}
	let tss = unsafe { &*(tss as *mut x86_64::structures::tss::TaskStateSegment) };
	gdt.add_entry(Descriptor::tss_segment(tss));

	unsafe {
		// Load the GDT for the current core.
		gdt.load();

		// Reload the segment descriptors
		load_cs(SegmentSelector::new(GDT_KERNEL_CODE, Ring::Ring0));
		load_ds(SegmentSelector::new(GDT_KERNEL_DATA, Ring::Ring0));
		load_es(SegmentSelector::new(GDT_KERNEL_DATA, Ring::Ring0));
		load_ss(SegmentSelector::new(GDT_KERNEL_DATA, Ring::Ring0));
		load_tr(SegmentSelector::new(GDT_FIRST_TSS, Ring::Ring0));
	}
}

pub extern "C" fn set_current_kernel_stack() {
	core_scheduler().set_current_kernel_stack();
}
