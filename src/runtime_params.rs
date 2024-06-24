//! Central parsing of the command-line parameters.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::str;

use ahash::RandomState;
use hashbrown::hash_map::Iter;
use hashbrown::HashMap;
use hermit_entry::boot_info::PlatformInfo;
use hermit_sync::OnceCell;

pub(crate) use crate::arch::kernel::{self, get_base_address, get_image_size, get_ram_address};
use crate::kernel::boot_info;

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
	env_vars: HashMap<String, String, RandomState>,
	args: Vec<String>,
	#[allow(dead_code)]
	mmio: Vec<String>,
}

/// Whether Hermit is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	matches!(boot_info().platform_info, PlatformInfo::Uhyve { .. })
}

impl Default for Cli {
	fn default() -> Self {
		let mut image_path = None;
		#[cfg(not(target_arch = "riscv64"))]
		let mut freq = None;
		let mut env_vars = HashMap::<String, String, RandomState>::with_hasher(
			RandomState::with_seeds(0, 0, 0, 0),
		);
		let mut args = Vec::new();
		let mut mmio = Vec::new();

		let words = shell_words::split(kernel::args().unwrap_or_default()).unwrap();
		debug!("cli_words = {words:?}");

		let mut words = words.into_iter();
		let expect_arg = |arg: Option<String>, name: &str| {
			arg.unwrap_or_else(|| {
				panic!("The argument '{name}' requires a value but none was supplied")
			})
		};
		while let Some(word) = words.next() {
			if word.as_str().starts_with("virtio_mmio.device=") {
				let v: Vec<&str> = word.as_str().split('=').collect();
				mmio.push(v[1].to_string());
				continue;
			}

			match word.as_str() {
				#[cfg(not(target_arch = "riscv64"))]
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
				"-mount" => {
					let gateway = expect_arg(words.next(), word.as_str());
					env_vars.insert(String::from("UHYVE_MOUNT"), gateway);
				}
				"--" => args.extend(&mut words),
				_ if image_path.is_none() => image_path = Some(word),
				word => warn!(
					"Found argument '{word}' which wasn't expected, or isn't valid in this context
			
 		If you tried to supply `{word}` as a value rather than a flag, use `-- {word}`"
				),
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

pub fn vars() -> Iter<'static, String, String> {
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
