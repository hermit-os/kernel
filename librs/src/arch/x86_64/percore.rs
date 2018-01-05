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

use arch::x86_64::processor;
use core::{mem, ptr};


extern "C" {
	static current_boot_id: u32;
	static percore_end0: u8;
	static percore_start: u8;
}

#[link_section = ".percore"]
#[no_mangle]
/// APIC ID of the current CPU Core.
pub static mut __core_id: u32 = 0;


pub trait PerCoreVariable {
	type VarType;
	unsafe fn per_core(&self) -> Self::VarType;
	unsafe fn set_per_core(&self, value: Self::VarType);
}

impl<T> PerCoreVariable for T {
	type VarType = T;

	#[inline]
	unsafe fn per_core(&self) -> T {
		let value: T;

		match mem::size_of::<T>() {
			4 => asm!("movl %gs:($1), $0" : "=r"(value) : "r"(self) :: "volatile"),
			8 => asm!("movq %gs:($1), $0" : "=r"(value) : "r"(self) :: "volatile"),
			_ => panic!("Invalid operand size for per_core"),
		}

		value
	}

	#[inline]
	unsafe fn set_per_core(&self, value: T) {
		match mem::size_of::<T>() {
			4 => asm!("movl $0, %gs:($1)" :: "r"(value), "r"(self) :: "volatile"),
			8 => asm!("movq $0, %gs:($1)" :: "r"(value), "r"(self) :: "volatile"),
			_ => panic!("Invalid operand size for set_per_core"),
		}
	}
}

#[inline]
pub fn core_id() -> u32 {
	unsafe { __core_id.per_core() }
}

pub fn init() {
	// Initialize the GS register, which is used for the per_core offset.
	unsafe {
		let size = &percore_end0 as *const u8 as usize - &percore_start as *const u8 as usize;
		let offset = ptr::read_volatile(&current_boot_id) as usize * size;
		processor::writegs(offset);
	}

	// Initialize the core ID.
	unsafe { __core_id.set_per_core(current_boot_id); }
}
