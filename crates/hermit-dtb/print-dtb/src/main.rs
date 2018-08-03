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

extern crate hermit_dtb;

use hermit_dtb::Dtb;
use std::env;
use std::fs::File;
use std::io;
use std::io::Read;
use std::str;

fn main() -> io::Result<()> {
	let args: Vec<String> = env::args().collect();

	if args.len() < 4 || (args[2] == "get_property" && args.len() < 5) {
		println!("Prints the contents of a Flattened Device Tree (.dtb) file");
		println!("and demonstrates the usage of the \"hermit-dtb\" crate.");
		println!("");
		println!("Usage: print-dtb <file> <function> <function parameter 1> <function parameter 2>");
		println!("");
		println!("  <function> can be one of: enum_subnodes, enum_properties, get_property");
		println!("    enum_subnodes needs 1 parameter: The path to enumerate subnodes");
		println!("    enum_properties needs 1 parameter: The path to enumerate properties");
		println!("    get_property needs 2 parameters: The path and the property name");
		println!("");
		println!("Examples:");
		println!("  ./print-dtb test.dtb enum_subnodes /");
		println!("  ./print-dtb test.dtb enum_properties /pl011");
		println!("    This finds e.g. the node /pl011@9000000 without knowing the address in advance.");
		println!("  ./print-dtb test.dtb get_property / compatible");
		println!("");
		println!("If you need a .dtb file to test, try out:");
		println!("  qemu-system-aarch64 -machine virt -nographic -machine dumpdtb=test.dtb");
		println!("");
		println!("If you want to convert a .dtb file into a human-readable .dts file, try out:");
		println!("  dtc -I dtb -o test.dts -O dts test.dtb");
		return Ok(());
	}

	let filename = &args[1];
	let function = &args[2];
	let parameter1 = &args[3];

	let mut f = File::open(filename)?;
	let mut buffer = Vec::<u8>::new();
	f.read_to_end(&mut buffer)?;

	let dtb = unsafe { Dtb::from_raw(buffer.as_slice().as_ptr()).expect(".dtb file has invalid header") };

	if function == "enum_subnodes" {
		for node in dtb.enum_subnodes(parameter1) {
			println!("{}", node);
		}
	} else if function == "enum_properties" {
		for node in dtb.enum_properties(parameter1) {
			println!("{}", node);
		}
	} else if function == "get_property" {
		let parameter2 = &args[4];
		let data_option = dtb.get_property(parameter1, parameter2);
		println!("{:?}", data_option);

		if let Some(data) = data_option {
			if let Ok(string) = str::from_utf8(data) {
				println!("As string: \"{}\"", string);
			}
		}
	} else {
		println!("Invalid function!");
	}

	Ok(())
}
