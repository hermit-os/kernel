// Copyright (c) 2020 Thomas Lambertz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::string::String;
use alloc::vec::Vec;

/// Splits a string at delimiter, except when its quoted with " or '. Useful for cmdline arguments.
/// Returns a vector of the split arguments, unquoted and unescaped.
pub fn tokenize(cmdline: &str, delimiter: char) -> Vec<String> {
	let mut current_token = String::with_capacity(cmdline.len());
	let mut chars = cmdline.chars();
	let mut tokens: Vec<String> = Vec::new();

	loop {
		// We have to use loop instead of for, since we advance the iterator in the loop during unquoting
		if let Some(c) = chars.next() {
			match c {
				_ if c == delimiter => {
					if !current_token.is_empty() {
						tokens.push(current_token.clone());
						current_token.clear();
					}
				}
				'"' | '\'' => {
					// Begin quoted string. Unquote will advance iterator!
					if let Ok(val) = unquote(c, &mut chars) {
						current_token.push_str(&val);
					}
				}
				_ => {
					current_token.push(c);
				}
			};
		} else {
			if !current_token.is_empty() {
				tokens.push(current_token);
			}
			break;
		}
	}
	tokens
}

/// Very simple unquote function for a string with unknown end. Used in conjunction with tokenize for parsing argument lists.
/// String is assumed to be ending with delimiter, but not starting, since the tokenize() already consumed the starting delimiter from the iterator.
/// Delimiter and a few common chars such as newline can be escaped with `\`
pub fn unquote(
	delimiter: char,
	chars: &mut impl Iterator<Item = char>,
) -> Result<String, &'static str> {
	let mut in_escape = false;
	let mut unquoted = String::with_capacity(255); // Avoid too many reallocs

	for x in chars {
		if in_escape {
			in_escape = false;
			unquoted.push(match x {
				'"' => '"',
				'\'' => '\'',
				'n' => '\n',
				'r' => '\r',
				't' => '\t',
				'\\' => '\\',
				_ if x == delimiter => delimiter,
				_ => return Err("Invalid escape char!"),
			});
		} else if x == '\\' {
			in_escape = true;
		} else if x == delimiter {
			// We reached the end of the quoted-string
			return Ok(unquoted);
		} else {
			unquoted.push(x);
		}
	}
	Err("Missing end-quote!")
}
