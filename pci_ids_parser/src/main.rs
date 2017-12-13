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

use std::env;
use std::fs::File;
use std::io::{Read, Write};


struct ProgrammingInterface {
	id: u8,
	name: String,
}

struct Subclass {
	id: u8,
	name: String,
	programming_interfaces: Vec<ProgrammingInterface>,
}

struct Class {
	id: u8,
	name: String,
	subclasses: Vec<Subclass>,
}

struct Device {
	id: u16,
	name: String,
}

struct Vendor {
	id: u16,
	name: String,
	devices: Vec<Device>,
}


fn parse_class(line: &str, classes: &mut Vec<Class>) {
	let class_id = u8::from_str_radix(&line[2..4], 16).unwrap();
	let class_name = &line[6..];
	classes.push(
		Class {
			id: class_id,
			name: class_name.to_string(),
			subclasses: Vec::new()
		}
	);
}

fn parse_subclass(line: &str, classes: &mut Vec<Class>) {
	let subclass_id = u8::from_str_radix(&line[1..3], 16).unwrap();
	let subclass_name = &line[5..];
	let last_class = classes.last_mut().expect("Found a subclass definition without a class");
	last_class.subclasses.push(
		Subclass {
			id: subclass_id,
			name: subclass_name.to_string(),
			programming_interfaces: Vec::new()
		}
	);
}

fn parse_programming_interface(line: &str, classes: &mut Vec<Class>) {
	let progif_id = u8::from_str_radix(&line[2..4], 16).unwrap();
	let progif_name = &line[6..];
	let last_class = classes.last_mut().expect("Found a progif definition without a class");
	let last_subclass = last_class.subclasses.last_mut().expect("Found a progif definition without a subclass");
	last_subclass.programming_interfaces.push(
		ProgrammingInterface {
			id: progif_id,
			name: progif_name.to_string(),
		}
	);
}

fn parse_vendor(line: &str, vendors: &mut Vec<Vendor>) {
	let vendor_id = u16::from_str_radix(&line[0..4], 16).unwrap();
	let vendor_name = &line[6..];
	vendors.push(
		Vendor {
			id: vendor_id,
			name: vendor_name.to_string(),
			devices: Vec::new()
		}
	);
}

fn parse_device(line: &str, vendors: &mut Vec<Vendor>) {
	let device_id = u16::from_str_radix(&line[1..5], 16).unwrap();
	let device_name = &line[7..];
	let last_vendor = vendors.last_mut().expect("Found a device definition without a vendor");
	last_vendor.devices.push(
		Device {
			id: device_id,
			name: device_name.to_string(),
		}
	);
}

fn sanitize(input: &String) -> String {
	let mut output = input.replace("\\", "\\\\");
	output = output.replace("\"", "\\\"");
	output
}

fn main() {
	let args: Vec<String> = env::args().collect();
	let input_filename = &args[1];
	let output_filename = &args[2];

	// Read the pci.ids input file into a string.
	let mut f = File::open(input_filename).expect("Could not find input file");
	let mut pci_ids = String::new();
	f.read_to_string(&mut pci_ids).expect("Something went wrong reading the input file");

	// Open the output file for writing.
	let mut f = File::create(output_filename).expect("Could not create output file");

	// Parse the input.
	let mut in_class = false;
	let mut classes: Vec<Class> = Vec::new();
	let mut vendors: Vec<Vendor> = Vec::new();

	for line in pci_ids.lines() {
		//println!("{}", line);
		let mut chars = line.chars();
		match chars.next() {
			None => continue,
			Some('#') => continue,
			Some('C') => {
				// A line like "C 01  Mass storage controller"
				in_class = true;
				parse_class(&line, &mut classes);
			},
			Some('\t') => {
				match chars.next() {
					Some('\t') => {
						// A line like "		30  XHCI" or "		0e11 4091  Smart Array 6i"
						if in_class {
							parse_programming_interface(&line, &mut classes);
						} else {
							// Subsystems are ignored.
						}
					},
					_ => {
						// A line like "	00  SCSI storage controller" or "	a0fa  BCM4210 iLine10 HomePNA 2.0"
						if in_class {
							parse_subclass(&line, &mut classes);
						} else {
							parse_device(&line, &mut vendors);
						}
					}
				}
			},
			_ => {
				// A line like "0e11  Compaq Computer Corporation"
				in_class = false;
				parse_vendor(&line, &mut vendors);
			}
		}
	}


	let mut output =
"
struct Class {
	id: u8,
	name: &'static str,
	subclasses: &'static [Subclass],
}

struct Subclass {
	id: u8,
	name: &'static str,
	programming_interfaces: &'static [ProgrammingInterface],
}

struct ProgrammingInterface {
	id: u8,
	name: &'static str,
}

struct Vendor {
	id: u16,
	name: &'static str,
	devices: &'static [Device],
}

struct Device {
	id: u16,
	name: &'static str,
}

".to_string();

	output += &format!("static CLASSES: &[Class] = &[\n");
	for c in &classes {
		output += &format!("\tClass {{ id: 0x{:02X}, name: \"{}\", subclasses: &[\n", c.id, c.name);

		for sc in &c.subclasses {
			output += &format!("\t\tSubclass {{ id: 0x{:02X}, name: \"{}\", programming_interfaces: &[\n", sc.id, sc.name);

			for pi in &sc.programming_interfaces {
				output += &format!("\t\t\tProgrammingInterface {{ id: 0x{:02X}, name: \"{}\" }},\n", pi.id, pi.name);
			}

			output += &format!("\t\t] }},\n");
		}

		output += &format!("\t] }},\n");
	}

	output += &format!("];\n\n");

	output += &format!("static VENDORS: &[Vendor] = &[\n");
	for v in &vendors {
		output += &format!("\tVendor {{ id: 0x{:04X}, name: \"{}\", devices: &[\n", v.id, sanitize(&v.name));

		for d in &v.devices {
			output += &format!("\t\tDevice {{ id: 0x{:04X}, name: \"{}\" }},\n", d.id, sanitize(&d.name));
		}

		output += &format!("\t] }},\n");
	}

	output += &format!("];\n");

	f.write_all(output.as_bytes()).unwrap();
}
