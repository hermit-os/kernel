// Copyright (c) 2020 Thomas Lambertz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::string::String;

/// Gets length of null terminated c string.
/// UNSAFE. Caller has to assert that string is null terminated!
pub unsafe fn c_strlen(c_str: *const u8) -> usize {
	let mut off = c_str;
	while *off != 0 {
		off = off.offset(1);
	}
	off as usize - c_str as usize
}

/// Converts null terminated c string into onwed rust utf8 string.
/// TODO: panics if not utf8. return error
pub unsafe fn c_str_to_str(c_str: *const u8) -> String {
	let len = c_strlen(c_str);
	core::str::from_utf8(core::slice::from_raw_parts(c_str, len))
		.unwrap()
		.into()
}

/// Gets length of null terminated c string. Limited to buffer length.
pub fn c_strbuflen(c_strbuf: &[u8]) -> usize {
	c_strbuf
		.iter()
		.position(|&s| s == 0)
		.unwrap_or(c_strbuf.len())
}

/// Converts (optional null terminated) c string buffer into onwed rust utf8 string.
/// Is safe, since input buffer has fixed length
/// TODO: panics if not utf8. return error
pub fn c_buf_to_str(c_strbuf: &[u8]) -> &str {
	let len = c_strbuflen(c_strbuf);
	core::str::from_utf8(&c_strbuf[0..len]).unwrap().into()
}
