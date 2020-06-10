#![no_std]
#![no_main]

extern crate hermit;
extern crate x86_64;

mod common;
use common::*;

/*
/// assert_eq but returns Result<(),&str> instead of panicking
/// no error message possible
/// adapted from libcore assert_eq macro
macro_rules! equals {
	($left:expr, $right:expr) => ({
		match (&$left, &$right) {
			(left_val, right_val) => {
				if !(*left_val == *right_val) {
					return Err(r#"assertion failed: `(left == right)`
  left: `{:?}`,
 right: `{:?}`"# &*left_val, &*right_val);
				}
				else { return Ok(()); }
			}
		}
	});
	($left:expr, $right:expr,) => ({
		$crate::assert_eq!($left, $right)
	});
}

macro_rules! n_equals {
	($left:expr, $right:expr) => ({
		match (&$left, &$right) {
			(left_val, right_val) => {
				if *left_val == *right_val {
					// The reborrows below are intentional. Without them, the stack slot for the
					// borrow is initialized even before the values are compared, leading to a
					// noticeable slow down.
					return Err(r#"assertion failed: `(left == right)`
  left: `{:?}`,
 right: `{:?}`"#, &*left_val, &*right_val);
				}
				else return Ok(());
			}
		}
	});
	($left:expr, $right:expr,) => {
		$crate::assert_ne!($left, $right)
	};
}
*/

//ToDo - add a testrunner so we can group multiple similar tests

//ToDo - Idea: pass some values into main - compute and print result to stdout
//ToDo - add some kind of assert like macro that returns a result instead of panicking, Err contains line number etc to pinpoint the issue
#[no_mangle]
pub fn main(args: Vec<String>) -> Result<(), ()> {
	let x = 25;
	let y = 310;
	let z = x * y;
	println!("25 * 310 = {}", z);
	assert_eq!(z, 7750);
	Ok(())
}
