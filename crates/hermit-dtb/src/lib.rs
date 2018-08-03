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

#![no_std]

// CRATES
extern crate byteorder;

// MACROS
macro_rules! align_down {
	($value:expr, $alignment:expr) => ($value & !($alignment - 1))
}

macro_rules! align_up {
	($value:expr, $alignment:expr) => (align_down!($value + ($alignment - 1), $alignment))
}

// IMPORTS
use byteorder::{BigEndian, ByteOrder};
use core::{cmp, mem, slice, str};

// FUNCTIONS
/// Get the length of a C-style null-terminated byte string, which is part of a larger slice.
/// The latter requirement makes this function safe to use.
fn c_strlen_on_slice(slice: &[u8]) -> usize {
	let mut end = slice;
	while !end.is_empty() && unsafe { end.get_unchecked(0) } != &0 {
		end = &end[1..];
	}

	(end.as_ptr() as usize - slice.as_ptr() as usize)
}

/// Get the token and advance the struct_slice to the next token.
fn parse_token(struct_slice: &mut &[u8]) -> u32 {
	let (token_slice, remaining_slice) = struct_slice.split_at(mem::size_of::<u32>());
	*struct_slice = remaining_slice;
	let token = BigEndian::read_u32(token_slice);

	token
}

/// Get the node name of a FDT_BEGIN_NODE token and advance the struct_slice to the next token.
fn parse_begin_node<'a>(struct_slice: &mut &'a [u8]) -> &'a str {
	let node_name_length = c_strlen_on_slice(struct_slice);
	let node_name = unsafe { str::from_utf8_unchecked(&struct_slice[..node_name_length]) };
	let aligned_length = align_up!(node_name_length + 1, mem::size_of::<u32>());
	*struct_slice = &struct_slice[aligned_length..];

	node_name
}

/// Get the property data length of a FDT_PROP token and advance the struct_slice to the property name offset.
fn parse_prop_data_length(struct_slice: &mut &[u8]) -> usize {
	let (property_length_slice, remaining_slice) = struct_slice.split_at(mem::size_of::<u32>());
	*struct_slice = remaining_slice;
	let property_length = BigEndian::read_u32(property_length_slice) as usize;

	property_length
}

/// Get the property name of a FDT_PROP token and advance the struct_slice to the next token.
fn parse_prop_name<'a>(struct_slice: &mut &[u8], strings_slice: &'a [u8]) -> &'a str {
	// Get the offset of the property name string inside strings_slice.
	let (property_name_offset_slice, remaining_slice) = struct_slice.split_at(mem::size_of::<u32>());
	*struct_slice = remaining_slice;
	let property_name_offset = BigEndian::read_u32(property_name_offset_slice) as usize;

	// Determine the length of that null-terminated string and return it.
	let property_name_slice = &strings_slice[property_name_offset..];
	let property_name_length = c_strlen_on_slice(property_name_slice);
	let property_name = unsafe { str::from_utf8_unchecked(&property_name_slice[..property_name_length]) };

	property_name
}

// CONSTANTS
const DTB_MAGIC: u32 = 0xD00DFEED;
const DTB_VERSION: u32 = 17;

const FDT_BEGIN_NODE: u32 = 0x00000001;
const FDT_END_NODE: u32   = 0x00000002;
const FDT_PROP: u32       = 0x00000003;
const FDT_NOP: u32        = 0x00000004;
const FDT_END: u32        = 0x00000009;


// STRUCTURES
#[repr(C)]
struct DtbHeader {
	magic: u32,
	totalsize: u32,
	off_dt_struct: u32,
	off_dt_strings: u32,
	off_mem_rsvmap: u32,
	version: u32,
	last_comp_version: u32,
	boot_cpuid_phys: u32,
	size_dt_strings: u32,
	size_dt_struct: u32,
}

pub struct Dtb<'a> {
	header: &'a DtbHeader,
	struct_slice: &'a [u8],
	strings_slice: &'a [u8],
}

impl<'a> Dtb<'a> {
	fn check_header(header: &DtbHeader) -> bool {
		(u32::from_be(header.magic) == DTB_MAGIC && u32::from_be(header.version) == DTB_VERSION)
	}

	pub unsafe fn from_raw(address: *const u8) -> Option<Self> {
		let header = & *(address as *const DtbHeader);
		if !Self::check_header(header) {
			return None;
		}

		let address = header as *const _ as usize + u32::from_be(header.off_dt_struct) as usize;
		let length = u32::from_be(header.size_dt_struct) as usize;
		let struct_slice = slice::from_raw_parts(address as *const u8, length);

		let address = header as *const _ as usize + u32::from_be(header.off_dt_strings) as usize;
		let length = u32::from_be(header.size_dt_strings) as usize;
		let strings_slice = slice::from_raw_parts(address as *const u8, length);

		Some(Self {
			header: header,
			struct_slice: struct_slice,
			strings_slice: strings_slice,
		})
	}

	pub fn enum_subnodes<'b>(&self, path: &'b str) -> EnumSubnodesIter<'a, 'b> {
		assert!(path.len() > 0);

		EnumSubnodesIter {
			struct_slice: self.struct_slice,
			path: path,
			nesting_level: 0,
			looking_on_level: 1,
		}
	}

	pub fn enum_properties<'b>(&self, path: &'b str) -> EnumPropertiesIter<'a, 'b> {
		assert!(path.len() > 0);

		EnumPropertiesIter {
			struct_slice: self.struct_slice,
			strings_slice: self.strings_slice,
			path: path,
			nesting_level: 0,
			looking_on_level: 1,
		}
	}

	pub fn get_property(&self, path: &str, property: &str) -> Option<&'a [u8]> {
		let mut struct_slice = self.struct_slice;
		let mut path = path;
		let mut nesting_level = 0;
		let mut looking_on_level = 1;

		while !struct_slice.is_empty() {
			let token = parse_token(&mut struct_slice);
			match token {
				FDT_BEGIN_NODE => {
					if path.is_empty() {
						// This is a subnode of the node we have been looking for.
						// The Flattened Device Tree Specification states that properties always precede subnodes, so we can stop.
						struct_slice = &[];
					} else {
						// The beginning of a node starts a new nesting level.
						nesting_level += 1;

						// Get the node information and advance the cursor to the next token.
						let node_name = parse_begin_node(&mut struct_slice);

						// We're only interested in this node if it is on the nesting level we are looking for.
						if looking_on_level == nesting_level {
							// path is advanced with every path component that matches, so we can compare it against
							// node_name using starts_with().
							// But path can either contain a full node name (like "uart@fe001000") or leave out the
							// unit address (like "uart@") to find the first UART device.
							// Therefore, get the minimum of both lengths and only call starts_with() on that length.
							let length_to_check = cmp::min(path.len(), node_name.len());
							let name_to_check = &node_name[..length_to_check];

							if node_name.is_empty() || path.starts_with(name_to_check) {
								// The current node is either the root node (node_name.is_empty()) or a matching path
								// component.
								// Advance path and the nesting level we are looking for.
								path = &path[length_to_check..];
								if path.starts_with("/") {
									// Skip the slash.
									path = &path[1..];
								}

								looking_on_level += 1;
							}
						}
					}
				},

				FDT_END_NODE => {
					// Finish this nesting level.
					nesting_level -= 1;

					if path.is_empty() {
						// If path is empty and we encounter the end of a nesting level, we have iterated over
						// all properties of the node we were looking for and can stop.
						struct_slice = &[];
					}
				},

				FDT_PROP => {
					// Get the property data length.
					let property_length = parse_prop_data_length(&mut struct_slice);
					let aligned_length = align_up!(property_length, mem::size_of::<u32>());

					if path.is_empty() {
						// We have reached the node we are looking for.
						// Now get the property_name to also check if this is the property we are looking for.
						let property_name = parse_prop_name(&mut struct_slice, self.strings_slice);

						if property_name == property {
							// It is, so get the data and return it.
							let property_data = &struct_slice[..property_length];
							return Some(property_data);
						} else {
							// It is not, so just advance the cursor.
							struct_slice = &struct_slice[aligned_length..];
						}
					} else {
						// Skip over the property name offset and data.
						struct_slice = &struct_slice[mem::size_of::<u32>()..];
						struct_slice = &struct_slice[aligned_length..];
					}
				},

				FDT_NOP => {
					// Nothing to be done for NOPs.
				},

				FDT_END => {
					// This marks the end of the device tree.
					struct_slice = &[];
				},

				_ => {
					panic!("get_property encountered an invalid token {:#010X} {} bytes before the end", token, struct_slice.len());
				}
			}
		}

		None
	}
}


pub struct EnumSubnodesIter<'a, 'b> {
	struct_slice: &'a [u8],
	path: &'b str,
	nesting_level: usize,
	looking_on_level: usize,
}

impl<'a, 'b> Iterator for EnumSubnodesIter<'a, 'b> {
	type Item = &'a str;

	fn next(&mut self) -> Option<&'a str> {
		while !self.struct_slice.is_empty() {
			let token = parse_token(&mut self.struct_slice);
			match token {
				FDT_BEGIN_NODE => {
					// The beginning of a node starts a new nesting level.
					self.nesting_level += 1;

					// Get the node information and advance the cursor to the next token.
					let node_name = parse_begin_node(&mut self.struct_slice);

					// We're only interested in this node if it is on the nesting level we are looking for.
					if self.looking_on_level == self.nesting_level {
						if self.path.is_empty() {
							// self.path is empty and we are on the right nesting level, so this is a subnode
							// we are looking for.
							return Some(node_name);
						} else {
							// self.path is advanced with every path component that matches, so we can compare it against
							// node_name using starts_with().
							// But self.path can either contain a full node name (like "uart@fe001000") or leave out the
							// unit address (like "uart@") to find the first UART device.
							// Therefore, get the minimum of both lengths and only call starts_with() on that length.
							let length_to_check = cmp::min(self.path.len(), node_name.len());
							let name_to_check = &node_name[..length_to_check];

							if node_name.is_empty() || self.path.starts_with(name_to_check) {
								// The current node is either the root node (node_name.is_empty()) or a matching path
								// component.
								// Advance self.path and the nesting level we are looking for.
								self.path = &self.path[length_to_check..];
								if self.path.starts_with("/") {
									// Skip the slash.
									self.path = &self.path[1..];
								}

								self.looking_on_level += 1;
							}
						}
					}
				},

				FDT_END_NODE => {
					// Finish this nesting level.
					self.nesting_level -= 1;

					if self.nesting_level < self.looking_on_level - 1 {
						// If the current nesting level is two levels below the level we are looking for,
						// we have finished enumerating the parent node and can stop.
						self.struct_slice = &[];
					}
				},

				FDT_PROP => {
					// EnumSubnodesIter is not interested in property information.
					// Get the property data length.
					let property_length = parse_prop_data_length(&mut self.struct_slice);
					let aligned_length = align_up!(property_length, mem::size_of::<u32>());

					// Skip over the property name offset and data.
					self.struct_slice = &self.struct_slice[mem::size_of::<u32>()..];
					self.struct_slice = &self.struct_slice[aligned_length..];
				},

				FDT_NOP => {
					// Nothing to be done for NOPs.
				},

				FDT_END => {
					// This marks the end of the device tree.
					self.struct_slice = &[];
				},

				_ => {
					panic!("EnumSubnodesIter encountered an invalid token {:#010X} {} bytes before the end", token, self.struct_slice.len());
				}
			}
		}

		None
	}
}


pub struct EnumPropertiesIter<'a, 'b> {
	struct_slice: &'a [u8],
	strings_slice: &'a [u8],
	path: &'b str,
	nesting_level: usize,
	looking_on_level: usize,
}

impl<'a, 'b> Iterator for EnumPropertiesIter<'a, 'b> {
	type Item = &'a str;

	fn next(&mut self) -> Option<&'a str> {
		while !self.struct_slice.is_empty() {
			let token = parse_token(&mut self.struct_slice);
			match token {
				FDT_BEGIN_NODE => {
					if self.path.is_empty() {
						// This is a subnode of the node we have been looking for.
						// The Flattened Device Tree Specification states that properties always precede subnodes, so we can stop.
						self.struct_slice = &[];
					} else {
						// The beginning of a node starts a new nesting level.
						self.nesting_level += 1;

						// Get the node information and advance the cursor to the next token.
						let node_name = parse_begin_node(&mut self.struct_slice);

						// We're only interested in this node if it is on the nesting level we are looking for.
						if self.looking_on_level == self.nesting_level {
							// self.path is advanced with every path component that matches, so we can compare it against
							// node_name using starts_with().
							// But self.path can either contain a full node name (like "uart@fe001000") or leave out the
							// unit address (like "uart@") to find the first UART device.
							// Therefore, get the minimum of both lengths and only call starts_with() on that length.
							let length_to_check = cmp::min(self.path.len(), node_name.len());
							let name_to_check = &node_name[..length_to_check];

							if node_name.is_empty() || self.path.starts_with(name_to_check) {
								// The current node is either the root node (node_name.is_empty()) or a matching path
								// component.
								// Advance self.path and the nesting level we are looking for.
								self.path = &self.path[length_to_check..];
								if self.path.starts_with("/") {
									// Skip the slash.
									self.path = &self.path[1..];
								}

								self.looking_on_level += 1;
							}
						}
					}
				},

				FDT_END_NODE => {
					// Finish this nesting level.
					self.nesting_level -= 1;

					if self.path.is_empty() {
						// If self.path is empty and we encounter the end of a nesting level, we have iterated over
						// all properties of the node we were looking for and can stop.
						self.struct_slice = &[];
					}
				},

				FDT_PROP => {
					// Get the property data length.
					let property_length = parse_prop_data_length(&mut self.struct_slice);
					let aligned_length = align_up!(property_length, mem::size_of::<u32>());

					if self.path.is_empty() {
						// We have reached the node we are looking for and this is a property to enumerate.
						// So get the property name, skip over the data, and return the name.
						let property_name = parse_prop_name(&mut self.struct_slice, self.strings_slice);
						self.struct_slice = &self.struct_slice[aligned_length..];
						return Some(property_name);
					} else {
						// Skip over the property name offset and data.
						self.struct_slice = &self.struct_slice[mem::size_of::<u32>()..];
						self.struct_slice = &self.struct_slice[aligned_length..];
					}
				},

				FDT_NOP => {
					// Nothing to be done for NOPs.
				},

				FDT_END => {
					// This marks the end of the device tree.
					self.struct_slice = &[];
				},

				_ => {
					panic!("EnumPropertiesIter encountered an invalid token {:#010X} {} bytes before the end", token, self.struct_slice.len());
				}
			}
		}

		None
	}
}


#[repr(C)]
struct DtbReserveEntry {
	address: u64,
	size: u64,
}
