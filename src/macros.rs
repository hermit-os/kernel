/// Prints to the standard output.
///
/// Adapted from [`std::print`].
///
/// [`std::print`]: https://doc.rust-lang.org/stable/std/macro.print.html
#[cfg(target_os = "none")]
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::console::_print(::core::format_args!($($arg)*));
    }};
}

/// Prints to the standard output, with a newline.
///
/// Adapted from [`std::println`].
///
/// [`std::println`]: https://doc.rust-lang.org/stable/std/macro.println.html
#[cfg(target_os = "none")]
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {{
        $crate::console::_print(::core::format_args!("{}\n", format_args!($($arg)*)));
    }};
}

/// Prints and returns the value of a given expression for quick and dirty
/// debugging.
// Copied from std/macros.rs
#[cfg(target_os = "none")]
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
#[cfg(not(any(
	target_arch = "riscv64",
	all(target_arch = "x86_64", feature = "newlib"),
	feature = "common-os"
)))]
macro_rules! kernel_function {
	($f:ident()) => {{
		use $crate::errno::ToErrno;

		// This propagates any unsafety requirements of `f` to the caller.
		if false {
			$f();
		}

		#[allow(unreachable_code)]
		#[allow(unused_unsafe)]
		unsafe {
			$crate::arch::switch::kernel_function0($f).set_errno()
		}
	}};

	($f:ident($arg1:expr)) => {{
		use $crate::errno::ToErrno;

		// This propagates any unsafety requirements of `f` to the caller.
		if false {
			$f($arg1);
		}

		#[allow(unreachable_code)]
		#[allow(unused_unsafe)]
		unsafe {
			$crate::arch::switch::kernel_function1($f, $arg1).set_errno()
		}
	}};

	($f:ident($arg1:expr, $arg2:expr)) => {{
		use $crate::errno::ToErrno;

		// This propagates any unsafety requirements of `f` to the caller.
		if false {
			$f($arg1, $arg2);
		}

		#[allow(unreachable_code)]
		#[allow(unused_unsafe)]
		unsafe {
			$crate::arch::switch::kernel_function2($f, $arg1, $arg2).set_errno()
		}
	}};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr)) => {{
		use $crate::errno::ToErrno;

		// This propagates any unsafety requirements of `f` to the caller.
		if false {
			$f($arg1, $arg2, $arg3);
		}

		#[allow(unreachable_code)]
		#[allow(unused_unsafe)]
		unsafe {
			$crate::arch::switch::kernel_function3($f, $arg1, $arg2, $arg3).set_errno()
		}
	}};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr)) => {{
		use $crate::errno::ToErrno;

		// This propagates any unsafety requirements of `f` to the caller.
		if false {
			$f($arg1, $arg2, $arg3, $arg4);
		}

		#[allow(unreachable_code)]
		#[allow(unused_unsafe)]
		unsafe {
			$crate::arch::switch::kernel_function4($f, $arg1, $arg2, $arg3, $arg4).set_errno()
		}
	}};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr)) => {{
		use $crate::errno::ToErrno;

		// This propagates any unsafety requirements of `f` to the caller.
		if false {
			$f($arg1, $arg2, $arg3, $arg4, $arg5);
		}

		#[allow(unreachable_code)]
		#[allow(unused_unsafe)]
		unsafe {
			$crate::arch::switch::kernel_function5($f, $arg1, $arg2, $arg3, $arg4, $arg5)
				.set_errno()
		}
	}};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr, $arg6:expr)) => {{
		use $crate::errno::ToErrno;

		// This propagates any unsafety requirements of `f` to the caller.
		if false {
			$f($arg1, $arg2, $arg3, $arg4, $arg5, $arg6);
		}

		#[allow(unreachable_code)]
		#[allow(unused_unsafe)]
		unsafe {
			$crate::arch::switch::kernel_function6($f, $arg1, $arg2, $arg3, $arg4, $arg5, $arg6)
				.set_errno()
		}
	}};
}

// TODO: Properly switch kernel stack with newlib
// https://github.com/hermit-os/kernel/issues/471
// TODO: Switch kernel stack on RISC-V
#[cfg(any(
	target_arch = "riscv64",
	all(target_arch = "x86_64", feature = "newlib"),
	feature = "common-os"
))]
macro_rules! kernel_function {
	($f:ident($($x:tt)*)) => {{
		use $crate::errno::ToErrno;
		#[allow(unreachable_code)]
		$f($($x)*).set_errno()
	}};
}

/// Returns the value of the specified environment variable.
///
/// The value is fetched from the current runtime environment and, if not
/// present, falls back to the same environment variable set at compile time
/// (might not be present as well).
#[allow(unused_macros)]
macro_rules! hermit_var {
	($name:expr) => {{
		use alloc::borrow::Cow;

		match crate::env::var($name) {
			Some(val) => Some(Cow::from(val)),
			None => option_env!($name).map(Cow::Borrowed),
		}
	}};
}

/// Tries to fetch the specified environment variable with a default value.
///
/// Fetches according to [`hermit_var`] or returns the specified default value.
#[allow(unused_macros)]
macro_rules! hermit_var_or {
	($name:expr, $default:expr) => {{
		hermit_var!($name).as_deref().unwrap_or($default)
	}};
}
