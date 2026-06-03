use alloc::boxed::Box;
use x86_64::instructions::tables;
use x86_64::registers::segmentation::{CS, DS, ES, SS, Segment};
#[cfg(feature = "common-os")]
use x86_64::structures::gdt::DescriptorFlags;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::paging::PageSize;
use x86_64::structures::tss::TaskStateSegment;

use super::interrupts::{IST_ENTRIES, IST_SIZE};
use crate::arch::BasePageSize;
use crate::arch::kernel::CURRENT_STACK;
use crate::arch::x86_64::kernel::core_local::{CoreLocal, core_scheduler};
use crate::mm::stack_alloc::allocate_stack;

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
	let rsp = CURRENT_STACK
		.lock()
		.take()
		.expect("no pre-reserved stack for kernel");
	tss.privilege_stack_table[0] = rsp.stack_end().into();
	drop(CoreLocal::get().kernel_stack.replace(Some(rsp.leak())));

	// Allocate all ISTs for this core.
	// Every task later gets its own IST, so the IST allocated here is only used by the Idle task.
	for i in 0..IST_ENTRIES {
		let size = if i == 0 {
			IST_SIZE
		} else {
			BasePageSize::SIZE as usize
		};

		let stack = allocate_stack(size);
		tss.interrupt_stack_table[i] = stack.stack_end().into();
		drop(CoreLocal::get().interrupt_stack_allocs[i].replace(Some(stack.leak())));
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
	{
		use x86_64::PhysAddr;
		use x86_64::registers::control::Cr3;
		use x86_64::structures::paging::PhysFrame;

		let root = crate::scheduler::get_root_page_table();
		let new_frame =
			PhysFrame::from_start_address(PhysAddr::new(root.try_into().unwrap())).unwrap();

		let (current_frame, val) = Cr3::read_raw();

		if current_frame != new_frame {
			unsafe {
				Cr3::write_raw(new_frame, val);
			}
		}
	}

	core_scheduler().set_current_kernel_stack();
}
