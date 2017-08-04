// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
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

use consts::*;
use x86::dtables::*;
use core::mem::size_of;

// This segment is a data segment
const GDT_FLAG_DATASEG: u8 = 0x02;
/// This segment is a code segment
const GDT_FLAG_CODESEG: u8 = 0x0a;
const GDT_FLAG_TSS: u8 = 0x09;
const GDT_FLAG_TSS_BUSY: u8 = 0x02;

const GDT_FLAG_SEGMENT: u8 = 0x10;
/// Privilege level: Ring 0
const GDT_FLAG_RING0: u8 = 0x00;
/// Privilege level: Ring 1
const GDT_FLAG_RING1: u8 = 0x20;
/// Privilege level: Ring 2
const GDT_FLAG_RING2: u8 = 0x40;
/// Privilege level: Ring 3
const GDT_FLAG_RING3: u8 = 0x60;
/// Segment is present
const GDT_FLAG_PRESENT: u8 = 0x80;
/// Segment was accessed
const GDT_FLAG_ACCESSED: u8 = 0x01;
/// Granularity of segment limit
/// - set: segment limit unit is 4 KB (page size)
/// - not set: unit is bytes
const GDT_FLAG_4K_GRAN: u8 = 0x80;
/// Default operand size
/// - set: 32 bit
/// - not set: 16 bit
const GDT_FLAG_16_BIT: u8 = 0x00;
const GDT_FLAG_32_BIT: u8 = 0x40;
const GDT_FLAG_64_BIT: u8 = 0x20;

// a TSS descriptor is twice larger than a code/data descriptor
const GDT_ENTRIES : usize = (6+MAX_CORES*2);
const MAX_IST : usize = 3;

// thread_local on a static mut, signals that the value of this static may
// change depending on the current thread.
static mut GDT: [GdtEntry; GDT_ENTRIES] = [GdtEntry::new(0, 0, 0, 0); GDT_ENTRIES];
static mut GDTR: DescriptorTablePointer = DescriptorTablePointer {
    limit: 0, //x((size_of::<GdtEntry>() * GDT_ENTRIES) - 1) as u16,
    base: 0 //GDT.as_ptr() as u64
};
static mut TSS_BUFFER: TssBuffer = TssBuffer::new();
static STACK_TABLE: [[IrqStack; MAX_IST]; MAX_CORES] = [[IrqStack::new(); MAX_IST]; MAX_CORES];

extern "C" {
	static boot_stack: [u8; MAX_CORES*KERNEL_STACK_SIZE*MAX_IST];
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct GdtEntry {
	/// Lower 16 bits of limit range
	limit_low: u16,
	/// Lower 16 bits of base address
	base_low: u16,
	/// middle 8 bits of base address
	base_middle: u8,
	/// Access bits
	access: u8,
	/// Granularity bits
	granularity: u8,
	/// Higher 8 bits of base address
	base_high: u8
}

impl GdtEntry {
    pub const fn new(base: u32, limit: u32, access: u8, gran: u8) -> Self {
        GdtEntry {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_middle: ((base >> 16) & 0xFF) as u8,
            access: access,
            granularity: (gran & 0xF0) as u8 | ((limit >> 16) & 0x0F) as u8,
            base_high: ((base >> 24) & 0xFF) as u8
        }
    }
}

/// definition of the tast state segment structure
#[derive(Copy, Clone)]
#[repr(C, packed)]
struct TaskStateSegment {
	res0: u16,	// reserved entries
	res1: u16,	// reserved entries
	rsp0: u64,
	rsp1: u64,
	rsp2: u64,
	res2: u32,	// reserved entries
	res3: u32,	// reserved entries
	ist1: u64,
	ist2: u64,
	ist3: u64,
	ist4: u64,
	ist5: u64,
	ist6: u64,
	ist7: u64,
	res4: u32,	// reserved entries
	res5: u32,	// reserved entries
	res6: u16,
	bitmap: u16,
}

impl TaskStateSegment {
    /// Creates a new TSS with zeroed privilege and interrupt stack table and a zero
    /// `iomap_base`.
    pub const fn new() -> TaskStateSegment {
        TaskStateSegment {
			res0: 0,	// reserved entries
			res1: 0,	// reserved entries
			rsp0: 0,
			rsp1: 0,
			rsp2: 0,
			res2: 0,	// reserved entries
			res3: 0,	// reserved entries
			ist1: 0,
			ist2: 0,
			ist3: 0,
			ist4: 0,
			ist5: 0,
			ist6: 0,
			ist7: 0,
			res4: 0,	// reserved entries
			res5: 0,	// reserved entries
			res6: 0,
			bitmap: 0,
        }
    }
}

// workaround to use th enew repr(align) feature
// currently, it is only supported by structs
// => map all TSS in a struct
#[repr(C, align(4096))]
struct TssBuffer {
	tss: [TaskStateSegment; MAX_CORES],
}

impl TssBuffer {
	pub const fn new() -> TssBuffer {
		TssBuffer {
			tss: [TaskStateSegment::new(); MAX_CORES],
		}
	}
}

// workaround to use th enew repr(align) feature
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
#[no_mangle]
pub extern fn gdt_install()
{
	unsafe {
		// Setup the GDT pointer and limit
		GDTR.limit = ((size_of::<GdtEntry>() * GDT_ENTRIES) - 1) as u16;
    	GDTR.base = (&GDT as *const _) as u64;

		let mut num: usize = 0;

		/* Our NULL descriptor */
		GDT[num] = GdtEntry::new(0, 0, 0, 0);
		num += 1;

		/*
		 * The second entry is our Code Segment. The base address
		 * is 0, the limit is 4 GByte, it uses 4KByte granularity,
		 * and is a Code Segment descriptor.
		 */
		GDT[num] = GdtEntry::new(0, 0,
			GDT_FLAG_RING0 | GDT_FLAG_SEGMENT | GDT_FLAG_CODESEG | GDT_FLAG_PRESENT, GDT_FLAG_64_BIT);
		num += 1;

		/*
		 * The third entry is our Data Segment. It's EXACTLY the
		 * same as our code segment, but the descriptor type in
		 * this entry's access byte says it's a Data Segment
		 */
		GDT[num] = GdtEntry::new(0, 0,
			GDT_FLAG_RING0 | GDT_FLAG_SEGMENT | GDT_FLAG_DATASEG | GDT_FLAG_PRESENT, 0);
		num += 1;

		/*
		 * Create code segment for 32bit user-space applications (ring 3)
		 */
		GDT[num] = GdtEntry::new(0, 0xFFFFFFFF,
			GDT_FLAG_RING3 | GDT_FLAG_SEGMENT | GDT_FLAG_CODESEG | GDT_FLAG_PRESENT, GDT_FLAG_32_BIT | GDT_FLAG_4K_GRAN);
		num += 1;

		/*
		 * Create data segment for 32bit user-space applications (ring 3)
		 */
		GDT[num] = GdtEntry::new(0, 0xFFFFFFFF,
			GDT_FLAG_RING3 | GDT_FLAG_SEGMENT | GDT_FLAG_DATASEG | GDT_FLAG_PRESENT, GDT_FLAG_32_BIT | GDT_FLAG_4K_GRAN);
		num += 1;

		/*
		 * Create code segment for 64bit user-space applications (ring 3)
		 */
		GDT[num] = GdtEntry::new(0, 0,
			GDT_FLAG_RING3 | GDT_FLAG_SEGMENT | GDT_FLAG_CODESEG | GDT_FLAG_PRESENT, GDT_FLAG_64_BIT);
		num += 1;

		/*
		 * Create data segment for 64bit user-space applications (ring 3)
		 */
		GDT[num] = GdtEntry::new(0, 0,
			GDT_FLAG_RING3 | GDT_FLAG_SEGMENT | GDT_FLAG_DATASEG | GDT_FLAG_PRESENT, 0);
		num += 1;

		/*
		 * Create TSS for each core (we use these segments for task switching)
		 */
		 for i in 0..MAX_CORES {
			 TSS_BUFFER.tss[i].rsp0 = (&(boot_stack[0]) as *const _) as u64;
			 TSS_BUFFER.tss[i].rsp0 += ((i+1) * KERNEL_STACK_SIZE - 0x10) as u64;
			 TSS_BUFFER.tss[i].ist1 = 0; // ist will created per task
			 TSS_BUFFER.tss[i].ist2 = (&(STACK_TABLE[i][2 /*IST number */ - 2]) as *const _) as u64;
			 TSS_BUFFER.tss[i].ist2 += (KERNEL_STACK_SIZE - 0x10) as u64;
			 TSS_BUFFER.tss[i].ist3 = (&(STACK_TABLE[i][3 /*IST number */ - 2]) as *const _) as u64;
			 TSS_BUFFER.tss[i].ist3 += (KERNEL_STACK_SIZE - 0x10) as u64;
			 TSS_BUFFER.tss[i].ist4 = (&(STACK_TABLE[i][4 /*IST number */ - 2]) as *const _) as u64;
			 TSS_BUFFER.tss[i].ist4 += (KERNEL_STACK_SIZE - 0x10) as u64;

			 let tss_ptr = &(TSS_BUFFER.tss[i]) as *const TaskStateSegment;
			 GDT[num+i*2] = GdtEntry::new(tss_ptr as u32, (size_of::<TaskStateSegment>()-1) as u32,
			 	GDT_FLAG_PRESENT | GDT_FLAG_TSS | GDT_FLAG_RING0, 0);
		 }

		 lgdt(&GDTR);
	}
}

#[no_mangle]
pub extern fn set_tss(rsp0: u64, ist1: u64)
{
	unsafe {
		TSS_BUFFER.tss[core_id!()].rsp0 = rsp0;
		TSS_BUFFER.tss[core_id!()].ist1 = ist1;
	}
}

#[no_mangle]
pub extern fn gdt_flush()
{
	unsafe { lgdt(&GDTR); }
}
