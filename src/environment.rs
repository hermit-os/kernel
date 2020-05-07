// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Determining and providing information about the environment (unikernel
//! vs. multi-kernel, hypervisor, etc.) as well as central parsing of the
//! command-line parameters.

#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::kernel::{
	get_base_address, get_cmdline, get_cmdsize, get_image_size, get_tls_filesz, get_tls_memsz,
	get_tls_start, is_single_kernel, is_uhyve,
};

#[cfg(target_arch = "aarch64")]
pub use arch::aarch64::kernel::{
	get_base_address, get_cmdline, get_cmdsize, get_image_size, is_single_kernel, is_uhyve,
};

use alloc::string::String;
use alloc::vec::Vec;
use core::{slice, str};
use util;

static mut COMMAND_LINE_CPU_FREQUENCY: u16 = 0;
static mut IS_PROXY: bool = false;
static mut COMMAND_LINE_APPLICATION: Option<Vec<String>> = None;
static mut COMMAND_LINE_PATH: Option<String> = None;

unsafe fn parse_command_line() {
	let cmdsize = get_cmdsize();
	if cmdsize == 0 {
		return;
	}

	// Convert the command-line into a Rust string slice.
	let cmdline = get_cmdline() as *const u8;
	let slice = slice::from_raw_parts(cmdline, cmdsize);
	let cmdline_str = str::from_utf8_unchecked(slice);

	// Split at spaces, but not while in quotes
	let tokens = util::tokenize(cmdline_str, ' ');
	debug!("Got cmdline tokens as {:?}", tokens);

	let mut tokeniter = tokens.into_iter();
	loop {
		if let Some(token) = tokeniter.next() {
			match token.as_str() {
				"-freq" => {
					let mhz_str = tokeniter.next().expect("Invalid -freq command line");
					COMMAND_LINE_CPU_FREQUENCY = mhz_str
						.parse()
						.expect("Could not parse -freq command line as number");
				}
				"-proxy" => {
					IS_PROXY = true;
				}
				"--" => {
					// Collect remaining arguments as applications argv
					COMMAND_LINE_APPLICATION = Some(tokeniter.collect());
					break;
				}
				_ if COMMAND_LINE_PATH.is_none() => {
					// Qemu passes in the kernel path (rusty-loader) as first argument
					COMMAND_LINE_PATH = Some(token)
				}
				_ => {
					warn!("Unknown cmdline option: {} [{}]", token, cmdline_str);
				}
			};
		} else {
			break;
		}
	}
}

/// Returns the cmdline argument passed in after "--"
pub fn get_command_line_argv() -> Option<&'static [String]> {
	unsafe { COMMAND_LINE_APPLICATION.as_deref() }
}

#[allow(dead_code)]
/// Returns the first cmdline argument, if not otherwise recognized. With qemu this is the host-path to the kernel (rusty-loader)
pub fn get_command_line_path() -> Option<&'static str> {
	unsafe { COMMAND_LINE_PATH.as_deref() }
}

pub fn init() {
	unsafe {
		parse_command_line();

		if is_uhyve() || is_single_kernel() {
			// We are running under uhyve or baremetal, which implies unikernel mode and no communication with "proxy".
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
