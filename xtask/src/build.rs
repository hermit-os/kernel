use std::collections::HashSet;
use std::env::{self, VarError};

use anyhow::Result;
use clap::Args;

use crate::cargo_build::CargoBuild;

/// Build the kernel.
#[derive(Args)]
pub struct Build {
	#[command(flatten)]
	cargo_build: CargoBuild,

	/// Enable the `-Z instrument-mcount` flag.
	#[arg(long)]
	pub instrument_mcount: bool,

	/// Enable the `-Z randomize-layout` flag.
	#[arg(long)]
	pub randomize_layout: bool,
}

impl Build {
	pub fn run(self) -> Result<()> {
		let sh = crate::sh()?;

		self.cargo_build.artifact.arch.install_for_build()?;

		let careful = match env::var_os("HERMIT_CAREFUL") {
			Some(val) if val == "1" => &["careful"][..],
			_ => &[],
		};

		eprintln!("Building kernel");
		let mut cargo = crate::cargo();
		cargo
			.args(careful)
			.arg("rustc")
			.arg("--crate-type=staticlib")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.cargo_build.artifact.arch.cargo_args())
			.args(self.cargo_build.cargo_build_args());

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		let build_archive = self.cargo_build.artifact.build_archive();
		let dist_archive = self.cargo_build.artifact.dist_archive();
		eprintln!(
			"Copying {} to {}",
			build_archive.as_ref().display(),
			dist_archive.as_ref().display()
		);
		sh.create_dir(dist_archive.as_ref().parent().unwrap())?;
		sh.copy_file(&build_archive, &dist_archive)?;

		eprintln!("Exporting symbols");
		self.export_syms()?;

		eprintln!("Building hermit-builtins");
		let mut cargo = crate::cargo();
		cargo
			.args(["build", "--release"])
			.arg("--manifest-path=hermit-builtins/Cargo.toml")
			.args(self.cargo_build.artifact.arch.builtins_cargo_args())
			.args(self.cargo_build.builtins_target_dir_arg());

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		eprintln!("Exporting hermit-builtins symbols");
		let builtins = self.cargo_build.artifact.builtins_archive();
		let builtin_symbols = sh.read_file("hermit-builtins/exports")?;
		builtins.retain_symbols(builtin_symbols.lines().collect::<HashSet<_>>())?;

		dist_archive.append(&builtins)?;

		eprintln!("Setting OSABI");
		dist_archive.set_osabi()?;

		eprintln!("Kernel available at {}", dist_archive.as_ref().display());
		Ok(())
	}

	fn cargo_encoded_rustflags(&self) -> Result<String> {
		let mut rustflags = hermit_rustflags_from_env().unwrap_or_default();

		if self.instrument_mcount {
			rustflags.push("-Zinstrument-mcount".to_owned());
			rustflags.push("-Cpasses=ee-instrument<post-inline>".to_owned());
		}

		if self.randomize_layout {
			rustflags.push("-Zrandomize-layout".to_owned())
		}

		rustflags.extend(
			self.cargo_build
				.artifact
				.arch
				.rustflags()
				.iter()
				.map(|&s| s.to_owned()),
		);

		Ok(rustflags.join("\x1f"))
	}

	fn export_syms(&self) -> Result<()> {
		let archive = self.cargo_build.artifact.dist_archive();

		let syscall_symbols = archive.syscall_symbols()?;
		let explicit_exports = ["_start", "__bss_start", "mcount", "runtime_entry"].into_iter();

		let symbols = explicit_exports.chain(syscall_symbols.iter().map(String::as_str));

		archive.retain_symbols(symbols.collect::<HashSet<_>>())?;

		Ok(())
	}
}

/// Gets Hermit-specific compiler flags from environment variables.
///
/// Adapted from Cargo's [`rustflags_from_env`](https://github.com/rust-lang/cargo/blob/2a7c4960677971f88458b0f8b461a866836dff59/src/cargo/core/compiler/build_context/target_info.rs#L815-L839).
fn hermit_rustflags_from_env() -> Option<Vec<String>> {
	match env::var("HERMIT_ENCODED_RUSTFLAGS") {
		Ok(a) => {
			if a.is_empty() {
				return Some(Vec::new());
			}
			return Some(a.split('\x1f').map(str::to_string).collect());
		}
		Err(VarError::NotPresent) => {}
		Err(VarError::NotUnicode(a)) => {
			panic!("HERMIT_ENCODED_RUSTFLAGS did not contain valid unicode data: {a:?}");
		}
	}

	match env::var("HERMIT_RUSTFLAGS") {
		Ok(a) => {
			let args = a
				.split(' ')
				.map(str::trim)
				.filter(|s| !s.is_empty())
				.map(str::to_string);
			return Some(args.collect());
		}
		Err(VarError::NotPresent) => {}
		Err(VarError::NotUnicode(a)) => {
			panic!("HERMIT_RUSTFLAGS did not contain valid unicode data: {a:?}");
		}
	}

	None
}
