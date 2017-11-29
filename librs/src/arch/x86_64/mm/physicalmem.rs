// Copyright (c) 2017 Colin Finck, RWTH Aachen University
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

use arch::x86_64::mm::paging::{BasePageSize, PageSize};
use collections::{FreeList, FreeListEntry};
use core::{mem, slice};
use mm;
use multiboot;


extern "C" {
	static limit: usize;
	static mb_info: multiboot::PAddr;
}


static PHYSICAL_FREE_LIST: FreeList = FreeList::new();


fn paddr_to_slice<'a>(p: multiboot::PAddr, sz: usize) -> Option<&'a [u8]> {
	unsafe {
		let ptr = mem::transmute(p);
		Some(slice::from_raw_parts(ptr, sz))
	}
}

fn detect_from_multiboot_info() -> bool {
	if unsafe { mb_info } == 0 {
		return false;
	}

	let mb = unsafe { multiboot::Multiboot::new(mb_info, paddr_to_slice).unwrap() };
	let all_regions = mb.memory_regions().expect("No memory regions supplied by multiboot information!");
	let ram_regions = all_regions.filter(|m|
		m.memory_type() == multiboot::MemoryType::Available &&
		m.base_address() + m.length() > mm::kernel_end_address() as u64
	);
	let mut locked_list = PHYSICAL_FREE_LIST.list.lock();

	for m in ram_regions {
		let start_address = if m.base_address() <= mm::kernel_start_address() as u64 {
			mm::kernel_end_address()
		} else {
			m.base_address() as usize
		};

		let entry = FreeListEntry {
			start: start_address,
			end: (m.base_address() + m.length()) as usize
		};
		locked_list.push(entry);
	}

	true
}

fn detect_from_limits() -> bool {
	if unsafe { limit } == 0 {
		return false;
	}

	let entry = FreeListEntry {
		start: mm::kernel_end_address(),
		end: unsafe { limit }
	};
	PHYSICAL_FREE_LIST.list.lock().push(entry);

	true
}

pub fn init() {
	detect_from_multiboot_info() || detect_from_limits() || panic!("Could not detect RAM");
}

pub fn allocate(size: usize) -> usize {
	assert!(size & (BasePageSize::SIZE - 1) == 0, "Size {:#X} is not aligned to {:#X}", size, BasePageSize::SIZE);

	let result = PHYSICAL_FREE_LIST.allocate(size);
	assert!(result.is_ok(), "Could not allocate {:#X} bytes of physical memory", size);
	result.unwrap()
}

pub fn deallocate(physical_address: usize, size: usize) {
	assert!(physical_address >= mm::kernel_end_address(), "Physical address {:#X} is not >= KERNEL_END_ADDRESS", physical_address);
	assert!(size & (BasePageSize::SIZE - 1) == 0, "Size {:#X} is not aligned to {:#X}", size, BasePageSize::SIZE);

	PHYSICAL_FREE_LIST.deallocate(physical_address, size);
}
