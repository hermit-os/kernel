/// This shell implementation is derived form
/// https://github.com/explodingcamera/pogos/tree/main/crates/simple-shell
use hermit_sync::Lazy;

use self::shell::*;
use crate::interrupts::print_statistics;

mod constants;
mod shell;
mod writer;

static mut SHELL: Lazy<Shell<'_>> = Lazy::new(|| {
	let print = |s: &str| print!("{}", s);
	let mut shell = Shell::new(print);

	shell.commands.insert(
		"help",
		ShellCommand {
			help: "Print this help message",
			func: |_, shell| {
				shell.print_help_screen();
				Ok(())
			},
			aliases: &["?", "h"],
		},
	);
	shell.commands.insert(
		"interrupts",
		ShellCommand {
			help: "Shows the number of received interrupts",
			func: |_, _shell| {
				print_statistics();
				Ok(())
			},
			aliases: &["i"],
		},
	);
	shell.commands.insert(
		"shutdown",
		ShellCommand {
			help: "Shutdown HermitOS",
			func: |_, _shell| {
				crate::shutdown(0);
			},
			aliases: &["s"],
		},
	);

	shell
});

pub(crate) fn init() {
	crate::executor::spawn(unsafe { SHELL.run_async() });
}
