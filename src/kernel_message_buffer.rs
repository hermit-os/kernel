// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Kernel Message Buffer for Multi-Kernel mode.
//! Can be read from the Linux side as no serial port is available.

use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam_utils::CachePadded;

const KMSG_SIZE: usize = 0x1000;

#[repr(C)]
struct KmsgSection {
	buffer: CachePadded<[u8; KMSG_SIZE + 1]>,
}

static mut KMSG: KmsgSection = KmsgSection {
	buffer: CachePadded::new([0; KMSG_SIZE + 1]),
};

static BUFFER_INDEX: CachePadded<AtomicUsize> = CachePadded::new(AtomicUsize::new(0));

pub fn write_byte(byte: u8) {
	let index = BUFFER_INDEX.fetch_add(1, Ordering::SeqCst);
	unsafe {
		core::ptr::write_volatile(&mut KMSG.buffer[index % KMSG_SIZE], byte);
	}
}
