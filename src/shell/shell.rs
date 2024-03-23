use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::future::{self, Future};
use core::task::{ready, Poll};

use super::constants::*;
use super::writer::*;
use crate::arch::kernel::COM1;

#[derive(Clone, Copy)]
pub struct ShellCommand<'a> {
	pub help: &'a str,
	pub func: fn(&[&str], &mut Shell<'a>) -> Result<(), &'a str>,
	pub aliases: &'a [&'a str],
}

pub struct Shell<'a> {
	pub history: Vec<String>,
	pub commands: BTreeMap<&'a str, ShellCommand<'a>>,
	pub command: String,
	pub cursor: usize,
	writer: Writer,
}

impl<'a> Shell<'a> {
	pub fn new(write: fn(&str)) -> Self {
		Self {
			history: Vec::new(),
			commands: BTreeMap::new(),
			command: String::new(),
			cursor: 0,
			writer: Writer::new(write),
		}
	}

	async fn get_char_async(&self) -> u8 {
		future::poll_fn(|cx| {
			let mut pinned = core::pin::pin!(COM1.async_lock());
			let mut guard = ready!(pinned.as_mut().poll(cx));
			if let Some(Some(c)) = guard.as_mut().map(|s| s.read()) {
				Poll::Ready(c)
			} else {
				cx.waker().wake_by_ref();
				Poll::Pending
			}
		})
		.await
	}

	#[allow(dead_code)]
	pub fn with_commands(mut self, mut commands: BTreeMap<&'a str, ShellCommand<'a>>) -> Self {
		self.commands.append(&mut commands);
		self
	}

	pub async fn run_async(&mut self) {
		self.print_prompt();

		loop {
			let c = self.get_char_async().await;
			match c {
				ESCAPE => self.handle_escape_async().await,
				_ => self.match_char(c),
			}
		}
	}

	fn match_char(&mut self, b: u8) {
		match b {
			CTRL_C => self.process_command("exit".to_string()),
			CTRL_L => self.handle_clear(),
			ENTER => self.handle_enter(),
			BACKSPACE => self.handle_backspace(),
			c if (32..=126).contains(&c) => {
				self.command.insert(self.cursor, c as char);
				self.cursor += 1;

				if self.cursor < self.command.len() {
					// Print the remaining text
					shell_print!(self.writer, "{}", &self.command[self.cursor - 1..]);
					// Move cursor to the correct position
					shell_print!(self.writer, "\x1b[{}D", self.command.len() - self.cursor);
				} else {
					shell_print!(self.writer, "{}", c as char);
				}
			}
			_ => {}
		}
	}

	fn handle_clear(&mut self) {
		self.clear_screen();
		self.print_prompt();
		shell_print!(self.writer, "{}", self.command);
		self.cursor = self.command.len();
	}

	fn handle_backspace(&mut self) {
		if self.cursor > 0 {
			self.command.remove(self.cursor - 1);
			self.cursor -= 1;
			shell_print!(self.writer, "\x08"); // Move cursor left
			shell_print!(self.writer, "{}", &self.command[self.cursor..]); // Print the remaining text
			shell_print!(self.writer, " "); // Clear last character
			shell_print!(
				self.writer,
				"\x1b[{}D",
				self.command.len() - self.cursor + 1
			);
			// Move cursor to the correct position
		}
	}

	fn handle_enter(&mut self) {
		shell_println!(self.writer, "");
		self.process_command(self.command.clone());
		self.history.push(self.command.clone());
		self.command.clear();
		self.cursor = 0;
		self.print_prompt();
	}

	async fn handle_escape_async(&mut self) {
		if self.get_char_async().await != CSI {
			return;
		}
		let b = self.get_char_async().await;
		self._handle_escape(b);
	}

	fn _handle_escape(&mut self, b: u8) {
		match b {
			CSI_UP => {}
			CSI_DOWN => {}
			CSI_RIGHT => {
				if self.cursor < self.command.len() {
					shell_print!(self.writer, "\x1b[1C");
					self.cursor += 1;
				}
			}
			CSI_LEFT => {
				if self.cursor > 0 {
					shell_print!(self.writer, "\x1b[1D");
					self.cursor -= 1;
				}
			}
			_ => {}
		}
	}

	fn process_command(&mut self, command: String) {
		let mut args = command.split_whitespace();
		let command = args.next().unwrap_or("");
		let args = args.collect::<Vec<_>>();

		for (name, shell_command) in &self.commands {
			if shell_command.aliases.contains(&command) || name == &command {
				return (shell_command.func)(&args, self).unwrap_or_else(|err| {
					shell_println!(self.writer, "{}: {}", command, err);
				});
			}
		}

		if command.is_empty() {
			return;
		}

		shell_println!(self.writer, "{}: command not found", command);
	}

	pub fn print_help_screen(&mut self) {
		shell_println!(self.writer, "available commands:");
		for (name, command) in &self.commands {
			shell_print!(self.writer, "  {:<12}{:<25}", name, command.help);
			if !command.aliases.is_empty() {
				shell_print!(self.writer, "    aliases: {}", command.aliases.join(", "));
			}
			shell_println!(self.writer, "");
		}
	}

	pub fn print_prompt(&mut self) {
		shell_print!(self.writer, "> ");
	}

	pub fn clear_screen(&mut self) {
		shell_print!(self.writer, "\x1b[2J\x1b[1;1H");
	}
}
