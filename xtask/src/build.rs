use std::env::{self, VarError};
use std::ffi::OsStr;

use anyhow::Result;
use clap::Args;
use xshell::cmd;

use crate::artifact::Artifact;

/// Build the kernel.
#[derive(Args)]
pub struct Build {
	#[command(flatten)]
	artifact: Artifact,

	/// Do not activate the `default` feature.
	#[arg(long)]
	pub no_default_features: bool,

	/// Space or comma separated list of features to activate.
	#[arg(long)]
	pub features: Vec<String>,

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

		eprintln!("Building kernel");
		cmd!(sh, "cargo build")
			.env("CARGO_ENCODED_RUSTFLAGS", self.cargo_encoded_rustflags()?)
			.args(&["--profile", self.artifact.profile()])
			.args(self.artifact.arch.cargo_args())
			.args(self.target_dir_args())
			.args(self.no_default_features_args())
			.args(self.features_args())
			.run()?;

		let build_archive = self.artifact.build_archive();
		let dist_archive = self.artifact.dist_archive();
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
			.args(self.artifact.arch.builtins_cargo_args())
			.args(self.target_dir_args())
			.run()?;

		eprintln!("Exporting hermit-builtins symbols");
		let builtins = self.artifact.builtins_archive();
		let builtin_symbols = sh.read_file("hermit-builtins/exports")?;
		builtins.retain_symbols(builtin_symbols.lines())?;

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
		// https://github.com/hermitcore/kernel/issues/200
		rustflags.push("-Zmutable-noalias=no");

		if self.instrument_mcount {
			rustflags.push("-Zinstrument-mcount");
		}

		if self.randomize_layout {
			rustflags.push("-Zrandomize-layout")
		}

		rustflags.extend(self.artifact.arch.rustflags());

		Ok(rustflags.join("\x1f"))
	}

	fn target_dir_args(&self) -> [&OsStr; 2] {
		["--target-dir".as_ref(), self.artifact.target_dir().as_ref()]
	}

	fn no_default_features_args(&self) -> &[&str] {
		if self.no_default_features {
			&["--no-default-features"]
		} else {
			&[]
		}
	}

	fn features_args(&self) -> impl Iterator<Item = &str> {
		self.features
			.iter()
			.flat_map(|feature| ["--features", feature.as_str()])
	}

	fn export_syms(&self) -> Result<()> {
		let archive = self.artifact.dist_archive();

		let syscall_symbols = archive.syscall_symbols()?;
		let explicit_exports = [
			"_start",
			"__bss_start",
			"runtime_entry",
			// lwIP functions (C runtime)
			"init_lwip",
			"lwip_read",
			"lwip_write",
			// lwIP rtl8139 driver
			"init_rtl8139_netif",
			"irq_install_handler",
			"virt_to_phys",
			"eoi",
		]
		.into_iter();

		let symbols = explicit_exports.chain(syscall_symbols.iter().map(String::as_str));

		archive.retain_symbols(symbols)?;

		Ok(())
	}
}
