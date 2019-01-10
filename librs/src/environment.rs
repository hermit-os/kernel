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

//! Determining and providing information about the environment (unikernel
//! vs. multi-kernel, hypervisor, etc.) as well as central parsing of the
//! command-line parameters.

#[cfg(target_arch="x86_64")]
pub use arch::x86_64::kernel::{is_uhyve,is_single_kernel,get_cmdsize,get_cmdline,get_image_size};

use core::{slice, str};

static mut COMMAND_LINE_CPU_FREQUENCY: u16 = 0;
static mut IS_PROXY: bool = false;


unsafe fn parse_command_line() {
	let cmdsize = get_cmdsize();
	if cmdsize == 0 {
		return;
	}

	// Convert the command-line into a Rust string slice.
	let cmdline = get_cmdline() as *const u8;
	let slice = slice::from_raw_parts(cmdline, cmdsize);
	let cmdline_str = str::from_utf8_unchecked(slice);

	// Check for the -freq option.
	if let Some(freq_index) = cmdline_str.find("-freq") {
		let cmdline_freq_str = cmdline_str.split_at(freq_index + "-freq".len()).1;
		let mhz_str = cmdline_freq_str.split(' ').next().expect("Invalid -freq command line");
		COMMAND_LINE_CPU_FREQUENCY = mhz_str.parse().expect("Could not parse -freq command line as number");
	}

	// Check for the -proxy option.
	IS_PROXY = cmdline_str.find("-proxy").is_some();
}

pub fn init() {
	unsafe {
		parse_command_line();

		if is_uhyve() {
			// We are running under uhyve, which implies unikernel mode and no communication with "proxy".
			IS_PROXY = false;
		} else {
			// We are running side-by-side to Linux, which implies communication with "proxy".
			IS_PROXY = true;
		}
	}
}

/// CPU Frequency in MHz if given through the -freq command-line parameter, otherwise zero.
pub fn get_command_line_cpu_frequency() -> u16 {
	unsafe { COMMAND_LINE_CPU_FREQUENCY }
}

/// Whether HermitCore shall communicate with the "proxy" application over a network interface.
/// Only valid after calling init()!
pub fn is_proxy() -> bool {
	unsafe { IS_PROXY }
}
