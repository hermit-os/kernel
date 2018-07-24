// Copyright (c) 2018 Colin Finck, RWTH Aachen University
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

use core::sync::atomic::spin_loop_hint;

extern "C" {
	static mut cpu_freq: u32;
}


pub struct FPUState {
	// TODO
}

impl FPUState {
	pub fn new() -> Self {
		Self {}
	}

	pub fn restore(&self) {
		// TODO
	}

	pub fn save(&self) {
		// TODO
	}
}


pub fn generate_random_number() -> Option<u32> {
	None
}

/// Search the most significant bit
#[inline(always)]
pub fn msb(value: u64) -> Option<u64> {
	if value > 0 {
		let ret: u64;
		let u64_bits = 64;
		unsafe { asm!("clz $0, $1; sub $0, $2, $0" : "=r"(ret) : "r"(value), "r"(u64_bits - 1) : "cc" : "volatile"); }
		Some(ret)
	} else {
		None
	}
}

/// The halt function stops the processor until the next interrupt arrives
pub fn halt() {
	unsafe {
		asm!("wfi" :::: "volatile");
	}
}

/// Shutdown the system
pub fn shutdown() -> ! {
	info!("Shutting down system");

	loop {
		halt();
	}
}

pub fn get_timer_ticks() -> u64 {
	// We simulate a timer with a 1 microsecond resolution by taking the CPU timestamp
	// and dividing it by the CPU frequency in MHz.
	0
}

pub fn get_frequency() -> u16 {
	0
}

#[inline]
pub fn get_timestamp() -> u64 {
	0
}

/// Delay execution by the given number of microseconds using busy-waiting.
#[inline]
pub fn udelay(usecs: u64) {
	let end = get_timestamp() + get_frequency() as u64 * usecs;
	while get_timestamp() < end {
		spin_loop_hint();
	}
}
