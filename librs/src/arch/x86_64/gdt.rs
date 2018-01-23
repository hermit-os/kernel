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

use arch::x86_64::percore::*;
use consts::*;
use core::mem;
use mm;
use spin;
use scheduler;
use scheduler::task::KernelStack;
use x86::bits64::segmentation::*;
use x86::bits64::task::*;
use x86::shared::PrivilegeLevel;
use x86::shared::dtables::{self, DescriptorTablePointer};
use x86::shared::task::*;

pub const GDT_KERNEL_CODE: u16 = 1;
pub const GDT_KERNEL_DATA: u16 = 2;
pub const GDT_FIRST_TSS:   u16 = 3;

/// A TSS descriptor is twice as large as a code/data descriptor.
const GDT_ENTRIES: usize = (3+MAX_CORES*2);

/// We use IST1 through IST4.
/// Each critical exception (NMI, Double Fault, Machine Check) gets a dedicated one while IST1 is shared for all other
/// interrupts. See also irq.rs.
const IST_ENTRIES: usize = 4;

// thread_local on a static mut, signals that the value of this static may
// change depending on the current thread.
static mut GDT: [SegmentDescriptor; GDT_ENTRIES] = [SegmentDescriptor::NULL; GDT_ENTRIES];
static mut GDTR: DescriptorTablePointer<SegmentDescriptor> = DescriptorTablePointer { base: 0 as *const SegmentDescriptor, limit: 0 };
static mut TSS_BUFFER: TssBuffer = TssBuffer::new();
static GDT_INIT: spin::Once<()> = spin::Once::new();

extern "C" {
	static boot_stack: *const u8;
}

// workaround to use the new repr(align) feature
// currently, it is only supported by structs
// => map all TSS in a struct
#[repr(align(4096))]
struct TssBuffer {
	tss: [TaskStateSegment; MAX_CORES],
}

impl TssBuffer {
	const fn new() -> TssBuffer {
		TssBuffer {
			tss: [TaskStateSegment::new(); MAX_CORES],
		}
	}
}


/// This will setup the special GDT
/// pointer, set up the entries in our GDT, and then
/// finally to load the new GDT and to update the
/// new segment registers
pub fn install() {
	unsafe {
		GDT_INIT.call_once(|| {
			// The NULL descriptor is already inserted as the first entry.

			// The second entry is a 64-bit Code Segment in kernel-space (Ring 0).
			// All other parameters are ignored.
			GDT[GDT_KERNEL_CODE as usize] = SegmentDescriptor::new_memory(0, 0, Type::Code(CODE_READ), false, PrivilegeLevel::Ring0, SegmentBitness::Bits64);

			// The third entry is a 64-bit Data Segment in kernel-space (Ring 0).
			// All other parameters are ignored.
			GDT[GDT_KERNEL_DATA as usize] = SegmentDescriptor::new_memory(0, 0, Type::Data(DATA_WRITE), false, PrivilegeLevel::Ring0, SegmentBitness::Bits64);

			// TODO: As soon as https://github.com/rust-lang/rust/issues/44580 is implemented, it should be possible to
			// implement "new" as "const fn" and do this call already in the initialization of GDTR.
			GDTR = DescriptorTablePointer::new(&GDT);
		});

		dtables::lgdt(&GDTR);

		// Reload the segment descriptors
		set_cs(SegmentSelector::new(GDT_KERNEL_CODE as u16, PrivilegeLevel::Ring0));
		load_ds(SegmentSelector::new(GDT_KERNEL_DATA as u16, PrivilegeLevel::Ring0));
		load_es(SegmentSelector::new(GDT_KERNEL_DATA as u16, PrivilegeLevel::Ring0));
		load_ss(SegmentSelector::new(GDT_KERNEL_DATA as u16, PrivilegeLevel::Ring0));
	}
}

pub fn create_tss() {
	let core_id = core_id() as usize;

	unsafe {
		// entry.asm has reserved space for a boot stack for each core.
		// Every task later gets its own stack, so this boot stack is only used by the Idle task on each core.
		// When switching to another task on this core, this entry is replaced.
		TSS_BUFFER.tss[core_id].rsp[0] = boot_stack as u64 + ((core_id+1) * KERNEL_STACK_SIZE - 0x10) as u64;

		// Allocate all ISTs for this core.
		// Every task later gets its own IST1, so the IST1 allocated here is only used by the Idle task.
		for i in 0..IST_ENTRIES {
			TSS_BUFFER.tss[core_id].ist[i] = mm::allocate(mem::size_of::<KernelStack>()) as u64 + KERNEL_STACK_SIZE as u64 - 0x10;
		}

		// Add this TSS to the GDT.
		let idx = GDT_FIRST_TSS as usize + core_id*2;
		GDT[idx..idx+2].copy_from_slice(&SegmentDescriptor::new_tss(&TSS_BUFFER.tss[core_id], PrivilegeLevel::Ring0));

		// Load it.
		let sel = SegmentSelector::new(idx as u16, PrivilegeLevel::Ring0);
		load_tr(sel);
	}
}

pub fn get_boot_stacks() -> (usize, usize) {
	let core_id = core_id() as usize;

	unsafe {
		let stack = TSS_BUFFER.tss[core_id].rsp[0] as usize;
		let ist = TSS_BUFFER.tss[core_id].ist[0] as usize;
		(stack, ist)
	}
}

#[no_mangle]
pub unsafe extern "C" fn set_current_kernel_stack() {
	let core_id = core_id() as usize;
	let core_scheduler = scheduler::get_scheduler(core_id as u32);
	let task = core_scheduler.get_current_task();
	let task_borrowed = task.borrow();

	TSS_BUFFER.tss[core_id].rsp[0] = (task_borrowed.stack as usize + KERNEL_STACK_SIZE - 0x10) as u64;
	TSS_BUFFER.tss[core_id].ist[0] = (task_borrowed.ist as usize + KERNEL_STACK_SIZE - 0x10) as u64;
}
