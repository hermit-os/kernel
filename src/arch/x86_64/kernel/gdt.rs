use alloc::alloc::alloc;
use alloc::boxed::Box;
use core::alloc::Layout;
use core::sync::atomic::Ordering;

use x86_64::instructions::tables;
use x86_64::registers::segmentation::{Segment, CS, DS, ES, SS};
#[cfg(feature = "common-os")]
use x86_64::structures::gdt::DescriptorFlags;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use super::interrupts::{IST_ENTRIES, IST_SIZE};
use super::scheduler::TaskStacks;
use super::CURRENT_STACK_ADDRESS;
use crate::arch::x86_64::kernel::core_local::{core_scheduler, CoreLocal};
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::config::KERNEL_STACK_SIZE;

pub fn add_current_core() {
	let gdt: &mut GlobalDescriptorTable = Box::leak(Box::new(GlobalDescriptorTable::new()));
	let kernel_code_selector = gdt.append(Descriptor::kernel_code_segment());
	let kernel_data_selector = gdt.append(Descriptor::kernel_data_segment());
	#[cfg(feature = "common-os")]
	{
		let _user_code32_selector =
			gdt.append(Descriptor::UserSegment(DescriptorFlags::USER_CODE32.bits()));
		let _user_data64_selector = gdt.append(Descriptor::user_data_segment());
		let _user_code64_selector = gdt.append(Descriptor::user_code_segment());
	}

	// Dynamically allocate memory for a Task-State Segment (TSS) for this core.
	let tss = Box::leak(Box::new(TaskStateSegment::new()));

	// Every task later gets its own stack, so this boot stack is only used by the Idle task on each core.
	// When switching to another task on this core, this entry is replaced.
	let rsp = CURRENT_STACK_ADDRESS.load(Ordering::Relaxed);
	let rsp = unsafe { rsp.add(KERNEL_STACK_SIZE - TaskStacks::MARKER_SIZE) };
	tss.privilege_stack_table[0] = VirtAddr::from_ptr(rsp);
	CoreLocal::get().kernel_stack.set(rsp);

	// Allocate all ISTs for this core.
	// Every task later gets its own IST, so the IST allocated here is only used by the Idle task.
	for i in 0..IST_ENTRIES {
		let size = if i == 0 {
			IST_SIZE
		} else {
			BasePageSize::SIZE as usize
		};

		let layout = Layout::from_size_align(size, BasePageSize::SIZE as usize).unwrap();
		let ist = unsafe { alloc(layout) };
		assert!(!ist.is_null());
		let ist_start = unsafe { ist.add(size - TaskStacks::MARKER_SIZE) };
		tss.interrupt_stack_table[i] = VirtAddr::from_ptr(ist_start);
	}

	CoreLocal::get().tss.set(tss);
	let tss_selector = gdt.append(Descriptor::tss_segment(tss));

	// Load the GDT for the current core.
	gdt.load();

	unsafe {
		// Reload the segment descriptors
		CS::set_reg(kernel_code_selector);
		DS::set_reg(kernel_data_selector);
		ES::set_reg(kernel_data_selector);
		SS::set_reg(kernel_data_selector);
		tables::load_tss(tss_selector);
	}
}

pub extern "C" fn set_current_kernel_stack() {
	#[cfg(feature = "common-os")]
	unsafe {
		let root = crate::scheduler::get_root_page_table();
		if root != x86::controlregs::cr3().try_into().unwrap() {
			x86::controlregs::cr3_write(root.try_into().unwrap());
		}
	}

	core_scheduler().set_current_kernel_stack();
}
