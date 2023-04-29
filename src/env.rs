//! Central parsing of the command-line parameters.

use alloc::string::String;
use alloc::vec::Vec;
use core::{slice, str};

use ahash::RandomState;
use hashbrown::hash_map::Iter;
use hashbrown::HashMap;
use hermit_entry::boot_info::PlatformInfo;
use hermit_sync::OnceCell;

pub use crate::arch::kernel::{
	get_base_address, get_image_size, get_ram_address, get_tls_align, get_tls_filesz,
	get_tls_memsz, get_tls_start,
};
use crate::arch::mm::VirtAddr;
use crate::kernel::boot_info;

static CLI: OnceCell<Cli> = OnceCell::new();

pub fn init() {
	CLI.set(Cli::default()).unwrap();
}

#[derive(Debug)]
struct Cli {
	#[allow(dead_code)]
	image_path: Option<String>,
	freq: Option<u16>,
	env_vars: HashMap<String, String, RandomState>,
	args: Vec<String>,
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	matches!(boot_info().platform_info, PlatformInfo::Uhyve { .. })
}

fn get_cmdsize() -> usize {
	match boot_info().platform_info {
		PlatformInfo::Multiboot { command_line, .. } => command_line
			.map(|command_line| command_line.len())
			.unwrap_or_default(),
		PlatformInfo::LinuxBootParams { command_line, .. } => command_line
			.map(|command_line| command_line.len())
			.unwrap_or_default(),
		PlatformInfo::Uhyve { .. } => 0,
	}
}

fn get_cmdline() -> VirtAddr {
	match boot_info().platform_info {
		PlatformInfo::Multiboot { command_line, .. } => VirtAddr(
			command_line
				.map(|command_line| command_line.as_ptr() as u64)
				.unwrap_or_default(),
		),
		PlatformInfo::LinuxBootParams { command_line, .. } => VirtAddr(
			command_line
				.map(|command_line| command_line.as_ptr() as u64)
				.unwrap_or_default(),
		),
		PlatformInfo::Uhyve { .. } => VirtAddr(0),
	}
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
		let mut env_vars = HashMap::<String, String, RandomState>::with_hasher(
			RandomState::with_seeds(0, 0, 0, 0),
		);
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
					env_vars.insert(String::from("HERMIT_IP"), ip);
				}
				"-mask" => {
					let mask = expect_arg(words.next(), word.as_str());
					env_vars.insert(String::from("HERMIT_MASK"), mask);
				}
				"-gateway" => {
					let gateway = expect_arg(words.next(), word.as_str());
					env_vars.insert(String::from("HERMIT_GATEWAY"), gateway);
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
	CLI.get().unwrap().freq
}

#[cfg(all(feature = "tcp", not(feature = "dhcpv4")))]
pub fn var(key: &str) -> Option<&String> {
	CLI.get().unwrap().env_vars.get(key)
}

pub fn vars() -> Iter<'static, String, String> {
	CLI.get().unwrap().env_vars.iter()
}

/// Returns the cmdline argument passed in after "--"
pub fn args() -> &'static [String] {
	CLI.get().unwrap().args.as_slice()
}
