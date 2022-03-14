//! Determining and providing information about the environment (unikernel
//! vs. multi-kernel, hypervisor, etc.) as well as central parsing of the
//! command-line parameters.

use alloc::{boxed::Box, string::String, vec::Vec};
use core::{slice, str};

use once_cell::race::OnceBox;

pub use crate::arch::kernel::{
	get_base_address, get_cmdline, get_cmdsize, get_image_size, get_ram_address, get_tls_align,
	get_tls_filesz, get_tls_memsz, get_tls_start, is_single_kernel, is_uhyve,
};

static ENV: OnceBox<Env> = OnceBox::new();

pub fn init() {
	ENV.set(Box::new(Env::default())).unwrap();
}

#[derive(Debug)]
struct Env {
	cli: Cli,
	is_proxy: bool,
}

impl Default for Env {
	fn default() -> Self {
		let cli = Cli::default();

		// Uhyve or baremetal implies unikernel mode and no communication with "proxy".
		// Else we are running side-by-side to Linux, which implies communication with "proxy".
		let is_proxy = !is_uhyve() && !is_single_kernel();

		Self { cli, is_proxy }
	}
}

#[derive(Debug)]
struct Cli {
	image_path: Option<String>,
	freq: Option<u16>,
	env_vars: Vec<String>,
	args: Vec<String>,
}

fn get_cmdline_str() -> &'static str {
	let cmdsize = get_cmdsize();
	let cmdline = get_cmdline().as_ptr::<u8>();
	if cmdline.is_null() {
		""
	} else {
		// SAFETY: cmdline and cmdsize are valid forever.
		let slice = unsafe { slice::from_raw_parts(cmdline, cmdsize) };
		str::from_utf8(slice).unwrap()
	}
}

impl Default for Cli {
	fn default() -> Self {
		let mut image_path = None;
		let mut freq = None;
		let mut env_vars = Vec::new();
		let mut args = Vec::new();

		let words = shell_words::split(get_cmdline_str()).unwrap();
		debug!("cli_words = {words:?}");

		let mut words = words.into_iter();
		let expect_arg = |arg: Option<String>, name: &str| {
			arg.unwrap_or_else(|| {
				panic!("The argument '{name}' requires a value but none was supplied")
			})
		};
		while let Some(word) = words.next() {
			match word.as_str() {
				"-freq" => {
					let s = expect_arg(words.next(), word.as_str());
					freq = Some(s.parse().unwrap());
				}
				"-ip" => {
					let ip = expect_arg(words.next(), word.as_str());
					env_vars.push(format!("HERMIT_IP={ip}"));
				}
				"-mask" => {
					let mask = expect_arg(words.next(), word.as_str());
					env_vars.push(format!("HERMIT_MASK={mask}"));
				}
				"-gateway" => {
					let gateway = expect_arg(words.next(), word.as_str());
					env_vars.push(format!("HERMIT_GATEWAY={gateway}"));
				}
				"--" => args.extend(&mut words),
				_ if image_path.is_none() => image_path = Some(word),
				word => panic!(
					"Found argument '{word}' which wasn't expected, or isn't valid in this context
			
 		If you tried to supply `{word}` as a value rather than a flag, use `-- {word}`"
				),
			};
		}

		Self {
			image_path,
			freq,
			env_vars,
			args,
		}
	}
}

/// CPU Frequency in MHz if given through the -freq command-line parameter.
pub fn freq() -> Option<u16> {
	ENV.get().unwrap().cli.freq
}

pub fn vars() -> &'static [String] {
	ENV.get().unwrap().cli.env_vars.as_slice()
}

/// Returns the cmdline argument passed in after "--"
pub fn args() -> &'static [String] {
	ENV.get().unwrap().cli.args.as_slice()
}

/// Whether HermitCore shall communicate with the "proxy" application over a network interface.
pub fn is_proxy() -> bool {
	ENV.get().unwrap().is_proxy
}
