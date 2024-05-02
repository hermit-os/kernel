use std::collections::HashSet;
use std::env::{self, VarError};

use anyhow::Result;
use clap::Args;
use xshell::cmd;

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
		cmd!(sh, "cargo")
			.args(careful)
			.arg("build")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(self.cargo_build.artifact.arch.cargo_args())
			.args(self.cargo_build.cargo_build_args())
			.run()?;

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
		cmd!(sh, "cargo build --release")
			.arg("--manifest-path=hermit-builtins/Cargo.toml")
			.args(self.cargo_build.artifact.arch.builtins_cargo_args())
			.args(self.cargo_build.target_dir_args())
			.run()?;

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
		let outer_rustflags = match env::var("CARGO_ENCODED_RUSTFLAGS") {
			Ok(s) => Some(s),
			Err(VarError::NotPresent) => None,
			Err(err) => return Err(err.into()),
		};
		let mut rustflags = outer_rustflags
			.as_deref()
			.map(|s| vec![s])
			.unwrap_or_default();

		// TODO: Re-enable mutable-noalias
		// https://github.com/hermit-os/kernel/issues/200
		rustflags.push("-Zmutable-noalias=no");

		if self.instrument_mcount {
			rustflags.push("-Zinstrument-mcount");
			rustflags.push("-Cpasses=ee-instrument<post-inline>");
		}

		if self.randomize_layout {
			rustflags.push("-Zrandomize-layout")
		}

		rustflags.extend(self.cargo_build.artifact.arch.rustflags());

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
