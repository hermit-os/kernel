use simple_shell::*;

use crate::interrupts::print_statistics;

fn read() -> Option<u8> {
	let mut buf = [0; 1];
	let len = crate::console::CONSOLE.lock().read(&mut buf).ok()?;
	if len > 0 { Some(buf[0]) } else { None }
}

pub(crate) fn init() {
	let (print, read) = (
		|s: &str| {
			print!("{s}");
			// flush buffer to see the input
			crate::console::CONSOLE.lock().flush();
		},
		read,
	);
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
			func: |_, _| {
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
			func: |_, _| crate::scheduler::shutdown(0),
			aliases: &["s"],
		},
	);

	// Also supports async
	crate::executor::spawn(async move { shell.run_async().await });
}
