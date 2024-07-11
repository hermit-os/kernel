use hermit_sync::Lazy;
use simple_shell::*;

use crate::arch::kernel::COM1;
use crate::console::CONSOLE;
use crate::interrupts::print_statistics;
use crate::io::Read;

static mut SHELL: Lazy<Shell<'_>> = Lazy::new(|| {
	let print = |s: &str| print!("{}", s);
	let read = || {
		let mut buf = [0];
		let n = CONSOLE.lock().read(&mut buf).unwrap();
		char::from_u32()
		(n == 1).then_some(buf[0])
	};
	let mut shell = Shell::new(print, read);

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
			func: |_, shell| {
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
			func: |_, shell| {
				crate::scheduler::shutdown(0);
				Ok(())
			},
			aliases: &["s"],
		},
	);

	shell
});

pub(crate) fn init() {
	// Also supports async
	crate::executor::spawn(unsafe { SHELL.run_async() });
}
