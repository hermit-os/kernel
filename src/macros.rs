/// Prints to the standard output.
///
/// Adapted from [`std::print`].
///
/// [`std::print`]: https://doc.rust-lang.org/stable/std/macro.print.html
#[cfg(target_os = "none")]
#[macro_export]
#[clippy::format_args]
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
#[clippy::format_args]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {{
        $crate::console::_print(::core::format_args!("{}\n", ::core::format_args!($($arg)*)));
    }};
}

/// Emergency output.
#[cfg(target_os = "none")]
#[macro_export]
#[clippy::format_args]
macro_rules! panic_println {
    () => {{
        $crate::console::_panic_print(::core::format_args!("\n"));
    }};
    ($($arg:tt)*) => {{
        $crate::console::_panic_print(::core::format_args!("{}\n", format_args!($($arg)*)));
    }};
}

#[cfg(not(target_os = "none"))]
#[macro_export]
#[clippy::format_args]
macro_rules! panic_println {
    ($($arg:tt)*) => {
        println!($($arg)*);
    };
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

/// Returns the value of the specified environment variable.
///
/// The value is fetched from the current runtime environment and, if not
/// present, falls back to the same environment variable set at compile time
/// (might not be present as well).
#[allow(unused_macros)]
macro_rules! hermit_var {
	($name:expr) => {{
		use alloc::borrow::Cow;

		match $crate::env::var($name) {
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
	($name:expr, $default:expr) => {
		hermit_var!($name).as_deref().unwrap_or($default)
	};
}
