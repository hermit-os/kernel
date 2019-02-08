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

//! Kernel Message Buffer for Multi-Kernel mode.
//! Can be read from the Linux side as no serial port is available.

use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};


const KMSG_SIZE: usize = 0x1000;

#[repr(C)]
struct KmsgSection {
	buffer: [u8; KMSG_SIZE + 1],
}

#[link_section = ".kmsg"]
static mut KMSG: KmsgSection = KmsgSection { buffer: [0; KMSG_SIZE + 1] };

static BUFFER_INDEX: AtomicUsize = AtomicUsize::new(0);


pub fn write_byte(byte: u8) {
	let index = BUFFER_INDEX.fetch_add(1, Ordering::SeqCst);
	unsafe {
		ptr::write_volatile(&mut KMSG.buffer[index % KMSG_SIZE], byte);
	}
}
