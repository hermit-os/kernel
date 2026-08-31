//! Inspection and manipulation of the kernel's environment.

mod start_info;

use alloc::borrow::{Cow, ToOwned};
use alloc::string::String;
use alloc::vec::Vec;
use core::str;

use ahash::RandomState;
use hashbrown::HashMap;
use hashbrown::hash_map::Iter;
use hermit_sync::OnceCell;
use shlex::Shlex;

pub use self::start_info::*;

static CLI: OnceCell<Cli> = OnceCell::new();

pub fn init() {
	CLI.set(Cli::default()).unwrap();
}

#[derive(Debug)]
struct Cli {
	#[allow(dead_code)]
	image_path: Option<String>,
	#[cfg(not(target_arch = "riscv64"))]
	freq: Option<u16>,
	env_vars: HashMap<Cow<'static, str>, String, RandomState>,
	args: Vec<String>,
	#[allow(dead_code)]
	mmio: Vec<String>,
}

impl Default for Cli {
	fn default() -> Self {
		let mut image_path = None;
		#[cfg(not(target_arch = "riscv64"))]
		let mut freq = None;
		let mut env_vars = HashMap::<Cow<'static, str>, String, RandomState>::with_hasher(
			RandomState::with_seeds(0, 0, 0, 0),
		);

		let args = start_info().bootargs().unwrap_or_default();
		info!("bootargs = {args}");
		let mut words = Shlex::new(args);

		let expect_arg = |arg: Option<String>, name: &str| {
			arg.unwrap_or_else(|| {
				panic!("The argument '{name}' requires a value but none was supplied")
			})
		};

		let mut args = Vec::new();
		let mut mmio = Vec::new();
		while let Some(word_owned) = words.next() {
			let word = word_owned.as_str();

			if let Some(arg) = word.strip_prefix("virtio_mmio.device=") {
				mmio.push(arg.to_owned());
				continue;
			}

			match word {
				#[cfg(not(target_arch = "riscv64"))]
				"-freq" => {
					let s = expect_arg(words.next(), word);
					freq = Some(s.parse().unwrap());
				}
				"-ip" => {
					let ip = expect_arg(words.next(), word);
					env_vars.insert(Cow::Borrowed("HERMIT_IP"), ip);
				}
				"-mask" => {
					let mask = expect_arg(words.next(), word);
					env_vars.insert(Cow::Borrowed("HERMIT_MASK"), mask);
				}
				"-gateway" => {
					let gateway = expect_arg(words.next(), word);
					env_vars.insert(Cow::Borrowed("HERMIT_GATEWAY"), gateway);
				}
				"-mount" => {
					let gateway = expect_arg(words.next(), word);
					env_vars.insert(Cow::Borrowed("UHYVE_MOUNT"), gateway);
				}
				"--" => args.extend(&mut words),
				_ => {
					if let Some(value) = word.strip_prefix("env=") {
						let Some((key, value)) = value.split_once('=') else {
							error!("could not parse bootarg: {word}");
							continue;
						};
						env_vars.insert(Cow::Owned(key.to_owned()), value.to_owned());
					} else if !word.contains('=') && image_path.is_none() {
						image_path = Some(word_owned);
					} else {
						error!("could not parse bootarg: {word}");
					}
				}
			};
		}

		Self {
			image_path,
			#[cfg(not(target_arch = "riscv64"))]
			freq,
			env_vars,
			args,
			#[allow(dead_code)]
			mmio,
		}
	}
}

/// CPU Frequency in MHz if given through the -freq command-line parameter.
#[cfg(not(target_arch = "riscv64"))]
pub fn freq() -> Option<u16> {
	CLI.get().unwrap().freq
}

#[allow(dead_code)]
pub fn var(key: &str) -> Option<&String> {
	CLI.get().unwrap().env_vars.get(key)
}

#[allow(dead_code)]
pub fn early_var(key: &str) -> Option<String> {
	match CLI.get() {
		Some(cli) => cli.env_vars.get(key).cloned(),
		None => {
			let prefix = format!("env={key}=");
			for i in Shlex::new(start_info().bootargs().unwrap_or_default()) {
				let i = i.as_str();
				if let Some(value) = i.strip_prefix(&prefix) {
					return Some(value.to_owned());
				}
			}
			None
		}
	}
}

pub fn vars() -> Iter<'static, Cow<'static, str>, String> {
	CLI.get().unwrap().env_vars.iter()
}

/// Returns the cmdline argument passed in after "--"
pub fn args() -> &'static [String] {
	CLI.get().unwrap().args.as_slice()
}

/// Returns the configuration of all mmio devices
#[allow(dead_code)]
pub fn mmio() -> &'static [String] {
	CLI.get().unwrap().mmio.as_slice()
}
