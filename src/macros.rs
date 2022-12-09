macro_rules! align_down {
	($value:expr, $alignment:expr) => {{
		use ::align_address::Align;
		($value).align_down($alignment)
	}};
}

macro_rules! align_up {
	($value:expr, $alignment:expr) => {{
		use ::align_address::Align;
		($value).align_up($alignment)
	}};
}

/// Print formatted text to our console.
///
/// From <http://blog.phil-opp.com/rust-os/printing-to-screen.html>, but tweaked
/// for HermitCore.
#[macro_export]
macro_rules! print {
	($($arg:tt)+) => ($crate::_print(::core::format_args!($($arg)+)));
}

/// Print formatted text to our console, followed by a newline.
#[macro_export]
macro_rules! println {
	() => (print!("\n"));
	($($arg:tt)+) => (print!("{}\n", ::core::format_args!($($arg)+)));
}

/// Prints and returns the value of a given expression for quick and dirty
/// debugging.
// Copied from std/macros.rs
#[macro_export]
macro_rules! dbg {
    // NOTE: We cannot use `concat!` to make a static string as a format argument
    // of `eprintln!` because `file!` could contain a `{` or
    // `$val` expression could be a block (`{ .. }`), in which case the `eprintln!`
    // will be malformed.
    () => {
        $crate::println!("[{}:{}]", ::core::file!(), ::core::line!())
    };
    ($val:expr $(,)?) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
                $crate::println!("[{}:{}] {} = {:#?}",
                    ::core::file!(), ::core::line!(), ::core::stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}

macro_rules! try_sys {
	($expr:expr $(,)?) => {
		match $expr {
			::core::result::Result::Ok(val) => val,
			::core::result::Result::Err(err) => {
				error!("{err}");
				return -1;
			}
		}
	};
}

/// Runs `f` on the kernel stack.
///
/// All arguments and return values have to fit into registers:
///
/// ```
/// assert!(mem::size_of::<T>() <= mem::size_of::<usize>());
/// ```
///
/// When working with bigger types, manually route the data over pointers:
///
/// ```
/// f(&arg1, &mut ret);
/// // instead of
/// let ret = f(arg);
/// ```
#[allow(unused_macro_rules)]
#[cfg(all(target_arch = "x86_64", not(feature = "newlib")))]
macro_rules! kernel_function {
	($f:ident()) => {
		$crate::arch::switch::kernel_function0($f)
	};

	($f:ident($arg1:expr)) => {
		$crate::arch::switch::kernel_function1($f, $arg1)
	};

	($f:ident($arg1:expr, $arg2:expr)) => {
		$crate::arch::switch::kernel_function2($f, $arg1, $arg2)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr)) => {
		$crate::arch::switch::kernel_function3($f, $arg1, $arg2, $arg3)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr)) => {
		$crate::arch::switch::kernel_function4($f, $arg1, $arg2, $arg3, $arg4)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr)) => {
		$crate::arch::switch::kernel_function5($f, $arg1, $arg2, $arg3, $arg4, $arg5)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr, $arg6:expr)) => {
		$crate::arch::switch::kernel_function6($f, $arg1, $arg2, $arg3, $arg4, $arg5, $arg6)
	};
}

// TODO: Properly switch kernel stack with newlib
// https://github.com/hermitcore/libhermit-rs/issues/471
#[cfg(any(
	target_arch = "aarch64",
	all(target_arch = "x86_64", feature = "newlib")
))]
macro_rules! kernel_function {
	($f:ident($($x:tt)*)) => {{
		$f($($x)*)
	}};
}
