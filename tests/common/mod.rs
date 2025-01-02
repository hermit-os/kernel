//! Common code for integration tests.
//!
//! See https://github.com/rust-lang/rust/blob/1.83.0/library/test/src/types.rs for reference.
//! See https://github.com/rust-lang/rust/issues/50297#issuecomment-524180479 for details.

use hermit::{print, println};

pub trait Testable {
	fn run(&self);
}

impl<T> Testable for T
where
	T: Fn(),
{
	fn run(&self) {
		print!("{}...\t", core::any::type_name::<T>());
		self();
		println!("[ok]");
	}
}

#[allow(dead_code)]
pub fn test_case_runner(tests: &[&dyn Testable]) {
	println!("Running {} tests", tests.len());
	for test in tests {
		test.run();
	}
	exit(false);
}

pub fn exit(failure: bool) -> ! {
	match failure {
		true => hermit::syscalls::sys_exit(1),
		false => hermit::syscalls::sys_exit(0),
	}
}

/// defines runtime_entry and passes arguments as Rust String to main method with signature:
/// `fn main(args: Vec<String>) -> Result<(), ()>;`
#[macro_export]
macro_rules! runtime_entry_with_args {
	() => {
		// ToDo: Maybe we could add a hard limit on the length of `s` to make this slightly safer?
		unsafe fn parse_str(s: *const u8) -> Result<alloc::string::String, ()> {
			use alloc::string::String;
			use alloc::vec::Vec;

			let mut vec: Vec<u8> = Vec::new();
			let mut off = s;
			while unsafe { *off } != 0 {
				vec.push(unsafe { *off });
				off = unsafe { off.offset(1) };
			}
			let str = String::from_utf8(vec);
			match str {
				Ok(s) => Ok(s),
				Err(_) => Err(()), //Convert error here since we might want to add another error type later
			}
		}

		#[unsafe(no_mangle)]
		extern "C" fn runtime_entry(
			argc: i32,
			argv: *const *const u8,
			_env: *const *const u8,
		) -> ! {
			use alloc::string::String;
			use alloc::vec::Vec;

			let mut str_vec: Vec<String> = Vec::new();
			let mut off = argv;
			for i in 0..argc {
				let s = unsafe { parse_str(*off) };
				unsafe {
					off = off.offset(1);
				}
				match s {
					Ok(s) => str_vec.push(s),
					Err(_) => println!(
						"Warning: Application argument {} is not valid utf-8 - Dropping it",
						i
					),
				}
			}

			let res = main(str_vec);
			match res {
				Ok(_) => common::exit(false),
				Err(_) => common::exit(true),
			}
		}
	};
}
