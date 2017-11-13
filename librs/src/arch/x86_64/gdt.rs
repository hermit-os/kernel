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

#![allow(dead_code)]
#![allow(private_no_mangle_fns)]

use arch::x86_64::percore::*;
use consts::*;
use spin;
use x86::bits64::segmentation::*;
use x86::bits64::task::*;
use x86::shared::PrivilegeLevel;
use x86::shared::dtables::{self, DescriptorTablePointer};
use x86::shared::task::*;

pub const GDT_KERNEL_CODE: u16 = 1;
pub const GDT_KERNEL_DATA: u16 = 2;
pub const GDT_FIRST_TSS:   u16 = 3;

/// A TSS descriptor is twice larger than a code/data descriptor.
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

/// Reserve space for all ISTs for each core.
static STACK_TABLE: [[IrqStack; IST_ENTRIES]; MAX_CORES] = [[IrqStack::new(); IST_ENTRIES]; MAX_CORES];

extern "C" {
	static boot_stack: *const u8;

	#[link_section = ".percore"]
	static __core_id: u32;
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

// workaround to use the new repr(align) feature
// currently, it is only supported by structs
// => map stacks in a struct
#[derive(Copy)]
#[repr(C, align(4096))]
struct IrqStack {
	buffer: [u8; KERNEL_STACK_SIZE],
}

impl Clone for IrqStack {
    fn clone(&self) -> IrqStack
	{
		*self
	}
}

impl IrqStack {
	pub const fn new() -> IrqStack {
		IrqStack {
			buffer: [0; KERNEL_STACK_SIZE],
		}
	}
}

/// This will setup the special GDT
/// pointer, set up the entries in our GDT, and then
/// finally to load the new GDT and to update the
/// new segment registers
pub fn install()
{
	unsafe {
		GDT_INIT.call_once(|| {
			// The NULL descriptor is already inserted as the first entry.

			// The second entry is a 64-bit Code Segment in kernel-space (Ring 0).
			// All other parameters are ignored.
			GDT[GDT_KERNEL_CODE as usize] = SegmentDescriptor::new_memory(0, 0, Type::Code(CODE_READ), false, PrivilegeLevel::Ring0, SegmentBitness::Bits64);

			// The third entry is a 64-bit Data Segment in kernel-space (Ring 0).
			// All other parameters are ignored.
			GDT[GDT_KERNEL_DATA as usize] = SegmentDescriptor::new_memory(0, 0, Type::Data(DATA_WRITE), false, PrivilegeLevel::Ring0, SegmentBitness::Bits64);

			// Create a TSS for each core.
			for i in 0..MAX_CORES {
				// entry.asm has reserved space for a boot stack for each core.
				// Every task has its own stack, so this boot stack is only used by the Idle task on each core.
				// When switching to another task on this core, this entry will be replaced.
				TSS_BUFFER.tss[i].rsp[0] = boot_stack as u64 + ((i+1) * KERNEL_STACK_SIZE - 0x10) as u64;

				// We have reserved space for the ISTs for each core.
				// They are only needed once for each core, not per task.
				for j in 0..IST_ENTRIES {
					TSS_BUFFER.tss[i].ist[j] = &STACK_TABLE[i][j] as *const _ as u64 + (KERNEL_STACK_SIZE - 0x10) as u64;
				}

				let idx = GDT_FIRST_TSS as usize + i*2;
				GDT[idx..idx+2].copy_from_slice(&SegmentDescriptor::new_tss(&TSS_BUFFER.tss[i], PrivilegeLevel::Ring0));
			}

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

		// Reload the task register to point to the proper TSS.
		let core_id = __core_id.per_core() as u16;
		let sel = SegmentSelector::new(GDT_FIRST_TSS as u16 + core_id*2, PrivilegeLevel::Ring0);
		load_tr(sel);
	}
}

#[no_mangle]
pub unsafe extern "C" fn set_tss(rsp: u64/*, ist: u64*/)
{
	let core_id = __core_id.per_core() as usize;
	TSS_BUFFER.tss[core_id].rsp[0] = rsp;
	//TSS_BUFFER.tss[core_id].ist[0] = ist;
}

#[no_mangle]
pub extern "C" fn gdt_install()
{
	install();
}
