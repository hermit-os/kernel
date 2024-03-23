use core::fmt::Write;

pub(crate) struct Writer {
	print: fn(&str),
}

impl core::fmt::Write for Writer {
	fn write_str(&mut self, s: &str) -> core::fmt::Result {
		(self.print)(s);
		Ok(())
	}
}

impl Writer {
	pub fn new(print: fn(&str)) -> Self {
		Self { print }
	}

	pub fn print(&mut self, t: &str) {
		self.write_str(t).unwrap();
	}

	pub fn print_args(&mut self, t: core::fmt::Arguments<'_>) {
		self.write_fmt(t).unwrap();
	}
}

macro_rules! shell_print {
    ($writer:expr, $fmt:literal$(, $($arg: tt)+)?) => {
        $writer.print_args(format_args!($fmt $(,$($arg)+)?))
    }
}

macro_rules! shell_println {
    ($writer:expr, $fmt:literal$(, $($arg: tt)+)?) => {{
        shell_print!($writer, $fmt $(,$($arg)+)?);
        $writer.print("\n");
    }};
    () => {
      $writer.print("\n");
    }
}

pub(crate) use {shell_print, shell_println};
