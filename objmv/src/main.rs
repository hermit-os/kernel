// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
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

#![feature(rustc_private)]

extern crate regex;
#[macro_use]
extern crate log;

use regex::RegexSet;
use std::str;
use std::env;
use std::process::Command;
use std::vec::Vec;

fn rename_sections(fname: String)
{
	let output = Command::new("objdump")
		.arg("-h")
		.arg(fname.to_string())
		.output()
		.expect("objdump failed to start");

	if output.status.success() {
		let mut args: Vec<String> = Vec::new();
		let re = RegexSet::new(&[r"^.text", r"^.data", r"^.bss"]).unwrap();
		let output_string = String::from_utf8_lossy(&output.stdout);
		let substrings = output_string.split_whitespace();

		for old_name in substrings {
			if re.is_match(old_name) {
				let mut new_name: String = ".k".to_owned();
				new_name.push_str(old_name.trim_left_matches('.'));
				//println!("{} {}", old_name, new_name);

				let mut cmd: String = String::new();
				cmd.push_str(old_name);
				cmd.push('=');
				cmd.push_str(&new_name);
				//println!("{}", cmd);

				args.push("--rename-section".to_string());
				args.push(cmd);
			}
		}

		if args.len() > 0 {
			let status = Command::new("objcopy")
				.args(args)
				.arg(fname.to_string())
				.status()
				.expect("objcopy failed to start");

			if !status.success() {
				warn!("Unable to rename sections in {}", fname);
			}
		}
	} else {
		warn!("Unable to determine section names in {}", fname);
	}
}

fn main() {
	let mut arguments: Vec<String> = env::args().collect();

	// remove unneeded programm name
	arguments.remove(0);

	for arg in arguments {
		rename_sections(arg);
	}
}
